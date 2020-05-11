use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, UpperHex};
use std::fs::File;
use std::io::Write;

mod generate;

/// Generates linker scripts and reset functions at build time
/// by building a description of the memory regions and sections in Rust.
///
/// Furthermore support safer usage of memory regions by allowing for
/// a double linking technique in cortex-m-rt-ld which ensures stack
/// and heap overflows cause hardware exceptions rather than overwriting
/// static data.
///
/// Based on ideas from Jorge Aparicio
/// * https://github.com/rust-embedded/cortex-m-rt/issues/164
/// * https://github.com/japaric/cortex-m-rt-ld

/// Machine word trait, used for alignment, templating, and sizing
pub trait Word: UpperHex + Clone + Display + Sized + Copy {}
impl Word for u32 {}
impl Word for u64 {}

/// Commonly used FLASH region name
pub const FLASH: &'static str = "FLASH";

/// Commonly used RAM region name
pub const RAM: &'static str = "RAM";

/// An ID given to a region
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegionID(String);

/// An ID given to a section
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SectionID(String);

/// LinkerError union type
#[derive(Debug)]
pub enum LinkerError {
    UnknownVMA(RegionID),
    UnknownLMA(RegionID),
    DuplicateRegion(String),
    DuplicateSection(String),
    MissingSection(String),
    IoError(std::io::Error),
}

impl fmt::Display for LinkerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            LinkerError::UnknownVMA(ref region_id) => {
                write!(f, "Region with ID {:?} used as VMA is unknown", region_id)
            }
            LinkerError::UnknownLMA(ref region_id) => {
                write!(f, "Region with ID {:?} used as LMA is unknown", region_id)
            }
            LinkerError::DuplicateRegion(ref name) => {
                write!(f, "Duplicate region, {:?} already defined", name)
            }
            LinkerError::DuplicateSection(ref name) => {
                write!(f, "Duplicate section, {:?} already defined", name)
            }
            LinkerError::MissingSection(ref name) => {
                write!(f, "Missing required section {:?}", name)
            }
            LinkerError::IoError(ref err) => write!(f, "{:?}", err),
        }
    }
}

impl Error for LinkerError {}

impl From<std::io::Error> for LinkerError {
    fn from(error: std::io::Error) -> Self {
        LinkerError::IoError(error)
    }
}

/// Result type alias
type Result<T> = std::result::Result<T, LinkerError>;

/// SectionSize describes the way in which a section should be sized
/// which maybe be linker, fixed, stack, or heap.
#[derive(Debug, Clone)]
enum SectionSize<W: Word> {
    /// The linker decides how large this section should be by introspecting the programs section size
    Linker,

    /// A fixed section size, this may overflow if not sized appropriately
    Fixed(W),

    /// Stack sizing will take the remaining regions space and locate the stack,
    /// with the stack start and stop reversed. The start of the stack is at the
    /// end of the space
    Stack,

    /// Heap sizing will take the remaining regions space. If both a
    /// stack and heap are assigned to the same region they will overlap.
    /// The start and end of the section will start at the lower address
    /// and end at the higher address like other sections.
    Heap,
}

/// Section describe where in memory certain parts of the program should be
/// placed, including if they are loaded from another Region, as well as
/// how they should be sized.
#[derive(Debug, Clone)]
struct Section<W: Word> {
    /// Priority given to the section when rendering a linker
    /// script. Lower values mean higher priority given to the
    /// section. Sections are placed in memory from the origin
    /// of a region in order of their priority.
    priority: i32,

    /// Name given to region
    name: String,

    /// Virtual memory region, region where the program will look
    /// for this section.
    vma: RegionID,

    /// Optional load memory region, region where the program will initially
    /// copy this section from.
    lma: Option<RegionID>,

    /// The size of the section can be fixed, automatic, or fill
    size: SectionSize<W>,

    /// Prefix defines a section with a name prefixed by the region
    /// for example if prefix is true, region name is "TCM" and
    /// name is "bss" the section name is .TCM.bss
    prefix: bool,

    /// Linker template preamble if needed (vector table needs this)
    linker_preamble: Option<String>,
}

impl<W: Word> Section<W> {
    fn heap(vma: RegionID) -> Self {
        Section {
            priority: i32::max_value(),
            size: SectionSize::Heap,
            prefix: false,
            name: String::from("heap"),
            vma: vma,
            lma: None,
            linker_preamble: None,
        }
    }

    fn stack(vma: RegionID) -> Self {
        Section {
            priority: i32::max_value() - 1,
            size: SectionSize::Stack,
            prefix: false,
            name: String::from("stack"),
            vma: vma,
            lma: None,
            linker_preamble: None,
        }
    }

    fn boot_config(size: W, name: &str, vma: RegionID) -> Self {
        Section {
            priority: -1,
            size: SectionSize::Fixed(size),
            prefix: false,
            name: String::from(name),
            vma: vma,
            lma: None,
            linker_preamble: None,
        }
    }

    fn vector_table(vma: RegionID, lma: Option<RegionID>) -> Self {
        Section {
            priority: 0,
            size: SectionSize::Linker,
            prefix: false,
            name: String::from("vector_table"),
            vma: vma,
            lma: lma,
            linker_preamble: Some(String::from("LONG(__start_stack);")),
        }
    }

    fn text(vma: RegionID, lma: Option<RegionID>) -> Self {
        Section {
            priority: 1,
            size: SectionSize::Linker,
            prefix: false,
            name: String::from("text"),
            vma: vma,
            lma: lma,
            linker_preamble: None,
        }
    }

    fn data(prefix: bool, vma: RegionID, lma: Option<RegionID>) -> Self {
        let priority = if prefix { 102 } else { 2 };
        Section {
            priority: priority,
            size: SectionSize::Linker,
            prefix: prefix,
            name: String::from("data"),
            vma: vma,
            lma: lma,
            linker_preamble: None,
        }
    }

    fn rodata(prefix: bool, vma: RegionID, lma: Option<RegionID>) -> Self {
        let priority = if prefix { 103 } else { 3 };
        Section {
            priority: priority,
            size: SectionSize::Linker,
            prefix: prefix,
            name: String::from("rodata"),
            vma: vma,
            lma: lma,
            linker_preamble: None,
        }
    }

    fn bss(prefix: bool, vma: RegionID, lma: Option<RegionID>) -> Self {
        let priority = if prefix { 104 } else { 4 };
        Section {
            priority: priority,
            size: SectionSize::Linker,
            prefix: prefix,
            name: String::from("bss"),
            vma: vma,
            lma: lma,
            linker_preamble: None,
        }
    }
}

/// Region description
#[derive(Debug, Clone)]
struct Region<W: Word> {
    name: String,
    origin: W,
    size: W,
}

/// LinkerScript is a buildable descriptor of memory regions,
/// common linker sections, and rules on what gets moved
/// (load memory address) where.
///
/// A sparse mapping of each regions virtual memory and load memory sections is
/// tracked.
#[derive(Debug)]
pub struct LinkerScript<W: Word> {
    regions: HashMap<String, Region<W>>,
    sections: HashMap<String, Section<W>>,
}

impl<W: Word> LinkerScript<W> {
    /// Create a new LinkerScript which can be mutate
    pub fn new() -> Self {
        LinkerScript {
            regions: HashMap::new(),
            sections: HashMap::new(),
        }
    }

    /// Add a named memory region
    pub fn region(&mut self, name: &str, origin: W, size: W) -> Result<RegionID> {
        let name = String::from(name);
        if self.regions.contains_key(&name) {
            return Err(LinkerError::DuplicateRegion(name.clone()));
        }
        let region = Region {
            name: name.clone(),
            origin: origin,
            size: size,
        };
        self.regions.insert(name.clone(), region);
        Ok(RegionID(name.clone()))
    }

    /// Required stack location
    ///
    /// The stack goes from the top address in the region downward.
    pub fn stack(&mut self, vma: RegionID) -> Result<SectionID> {
        let section = Section::stack(vma);
        self.add_section(section)
    }

    /// Optional heap location and size
    ///
    /// Places the heap as the last section in a region with addresses
    /// going higher available to it.
    pub fn heap(&mut self, vma: RegionID) -> Result<SectionID> {
        let section = Section::heap(vma);
        self.add_section(section)
    }

    /// Optional boot config section which is placed before the vector table.
    /// This is commonly used in devices which boot from external memory devices
    /// and require a configuration section to describe the device they are
    /// booting from and how to proceed.
    pub fn boot_config(&mut self, size: W, name: &str, vma: RegionID) -> Result<SectionID> {
        let section = Section::boot_config(size, name, vma);
        self.add_section(section)
    }

    /// Required vector table, by default this is placed at the beginning
    /// of the text section but maybe useful in some instances to load to a
    /// different location. By using this VTOR is updated
    pub fn vector_table(&mut self, vma: RegionID, lma: Option<RegionID>) -> Result<SectionID> {
        let section = Section::vector_table(vma, lma);
        self.add_section(section)
    }

    /// Required text section
    pub fn text(&mut self, vma: RegionID, lma: Option<RegionID>) -> Result<SectionID> {
        let section = Section::text(vma, lma);
        self.add_section(section)
    }

    /// Required data section
    pub fn data(
        &mut self,
        prefix: bool,
        vma: RegionID,
        lma: Option<RegionID>,
    ) -> Result<SectionID> {
        let section = Section::data(prefix, vma, lma);
        self.add_section(section)
    }

    /// Required rodata section
    pub fn rodata(
        &mut self,
        prefix: bool,
        vma: RegionID,
        lma: Option<RegionID>,
    ) -> Result<SectionID> {
        let section = Section::rodata(prefix, vma, lma);
        self.add_section(section)
    }

    /// Required bss section
    pub fn bss(&mut self, prefix: bool, vma: RegionID, lma: Option<RegionID>) -> Result<SectionID> {
        let section = Section::bss(prefix, vma, lma);
        self.add_section(section)
    }

    fn add_section(&mut self, section: Section<W>) -> Result<SectionID> {
        let name = section.name.clone();
        if self.sections.contains_key(&name) {
            return Err(LinkerError::DuplicateSection(name.clone()));
        }
        self.sections.insert(name.clone(), section);
        Ok(SectionID(name.clone()))
    }

    /// Generate a linker script and matching reset module
    /// which correctly initializes sections.
    ///
    /// The function places a linker script file, called `link.x`, in
    /// the current working directory.
    pub fn generate(self) -> Result<()> {
        let mut link_x = File::create("link.x")?;
        self.write(&mut link_x)
    }

    /// Write the linker script into the writer, `link_x`
    pub fn write<Wr: Write>(self, link_x: &mut Wr) -> Result<()> {
        const REQ_SEC_NAMES: [&str; 6] = ["stack", "vector_table", "text", "data", "rodata", "bss"];
        for req_sec_name in REQ_SEC_NAMES.iter() {
            let name = String::from(*req_sec_name);
            if !self.sections.contains_key(&name) {
                return Err(LinkerError::MissingSection(name));
            }
        }
        generate::link::render(&self, link_x)?;
        Ok(())
        //let reset = generate::reset::render(&self)?;
        //let mut reset_rs = File::create("reset.rs")?;
        //reset_rs.write_all(&reset)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generate_ok() {
        let mut ls = LinkerScript::<u32>::new();
        let flash = ls.region(FLASH, 0x0, 512).unwrap();
        let ram = ls.region(RAM, 0x20000000, 128).unwrap();
        ls.stack(ram.clone()).unwrap();
        ls.heap(ram.clone()).unwrap();
        ls.boot_config(512, "fcb", flash.clone()).unwrap();
        ls.vector_table(flash.clone(), Some(ram.clone())).unwrap();
        ls.text(flash.clone(), Some(ram.clone())).unwrap();
        ls.data(false, flash.clone(), Some(ram.clone())).unwrap();
        ls.rodata(false, flash.clone(), None).unwrap();
        ls.bss(false, flash.clone(), Some(ram.clone())).unwrap();
        ls.generate().unwrap();
    }

    //
    // The 'rejects_*' tests show that we reject linker scripts that are missing
    // our required sections.
    //

    #[derive(PartialEq, Eq)]
    enum Required {
        Stack,
        VectorTable,
        Text,
        Data,
        ROData,
        Bss,
    }

    impl ToString for Required {
        fn to_string(&self) -> String {
            ToString::to_string(match self {
                Required::Stack => "stack",
                Required::VectorTable => "vector_table",
                Required::Text => "text",
                Required::Data => "data",
                Required::ROData => "rodata",
                Required::Bss => "bss",
            })
        }
    }

    fn reject_missing(required: Required) {
        let mut ls = LinkerScript::<u32>::new();
        let flash = ls.region(FLASH, 0x0, 512).unwrap();
        let ram = ls.region(RAM, 0x20000000, 128).unwrap();
        if Required::Stack != required {
            ls.stack(ram.clone()).unwrap();
        }
        if Required::VectorTable != required {
            ls.vector_table(flash.clone(), Some(ram.clone())).unwrap();
        }
        if Required::Text != required {
            ls.text(flash.clone(), Some(ram.clone())).unwrap();
        }
        if Required::Data != required {
            ls.data(false, flash.clone(), Some(ram.clone())).unwrap();
        }
        if Required::ROData != required {
            ls.rodata(false, flash.clone(), None).unwrap();
        }
        if Required::Bss != required {
            ls.bss(false, flash.clone(), Some(ram.clone())).unwrap();
        }
        match ls.generate() {
            Err(LinkerError::MissingSection(section)) if section == required.to_string() => {}
            result => panic!(
                "Expected missing {}, but got {:?}",
                required.to_string(),
                result
            ),
        };
    }

    #[test]
    fn rejects_missing_vector_table() {
        reject_missing(Required::VectorTable);
    }

    #[test]
    fn rejects_missing_stack() {
        reject_missing(Required::Stack);
    }

    #[test]
    fn rejects_missing_text() {
        reject_missing(Required::Text);
    }

    #[test]
    fn rejects_missing_data() {
        reject_missing(Required::Data);
    }

    #[test]
    fn rejects_missing_rodata() {
        reject_missing(Required::ROData);
    }

    #[test]
    fn rejects_missing_bss() {
        reject_missing(Required::Bss);
    }
}
