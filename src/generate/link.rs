use crate::{LinkerScript, Section, SectionSize, Word};
use std::io::{Error, Write};

/// render a linker sized section
fn render_linker_section<W: Word, Wr: Write>(
    out: &mut Wr,
    section: &Section<W>,
) -> Result<(), Error> {
    writeln!(out, "\t.{} :", section.name)?;
    writeln!(out, "\t{{")?;
    writeln!(out, "\t\t. = ALIGN({});", std::mem::align_of::<W>())?;
    writeln!(out, "\t\t__start_{} = .;", section.name)?;
    if let Some(linker_preamble) = &section.linker_preamble {
        writeln!(out, "\t\t{}", linker_preamble)?;
    }
    writeln!(out, "\t\t*(.{} .{}.*);", section.name, section.name)?;
    writeln!(out, "\t\t. = ALIGN({});", std::mem::align_of::<W>())?;
    writeln!(out, "\t\t__end_{} = .;", section.name)?;
    if let Some(lma) = &section.lma {
        writeln!(out, "\t}} > {} AT> {}", section.vma.0, lma.0)?;
        writeln!(
            out,
            "\t__load_{} = LOADADDR(.{});",
            section.name, section.name
        )?;
        writeln!(
            out,
            "\t__{}_used = __{}_used + SIZEOF(.{});",
            section.vma.0, section.vma.0, section.name
        )?;
        writeln!(
            out,
            "\t__{}_used = __{}_used + SIZEOF(.{});",
            lma.0, lma.0, section.name
        )?;
    } else {
        writeln!(out, "\t}} > {}", section.vma.0)?;
        writeln!(
            out,
            "\t__{}_used = __{}_used + SIZEOF(.{});",
            section.vma.0, section.vma.0, section.name
        )?;
    }
    writeln!(out, "")?;
    Ok(())
}

/// render a heap section
fn render_heap_section<W: Word, Wr: Write>(
    out: &mut Wr,
    section: &Section<W>,
) -> Result<(), Error> {
    writeln!(out, "\t.{} :", section.name)?;
    writeln!(out, "\t{{")?;
    writeln!(
        out,
        "\t\t. = __{}_origin + __{}_used;",
        section.vma.0, section.vma.0
    )?;
    writeln!(out, "\t\t. = ALIGN({});", std::mem::align_of::<W>())?;
    writeln!(out, "\t\t__start_{} = .;", section.name)?;
    writeln!(
        out,
        "\t\t. = __{}_origin + __{}_size;",
        section.vma.0, section.vma.0
    )?;
    writeln!(out, "\t\t__end_{} = .;", section.name)?;
    writeln!(out, "\t}} > {}", section.vma.0)?;
    writeln!(out, "")?;
    Ok(())
}

/// render a heap section
fn render_stack_section<W: Word, Wr: Write>(
    out: &mut Wr,
    section: &Section<W>,
) -> Result<(), Error> {
    writeln!(out, "\t.{} :", section.name)?;
    writeln!(out, "\t{{")?;
    writeln!(
        out,
        "\t\t. = __{}_origin + __{}_used;",
        section.vma.0, section.vma.0
    )?;
    writeln!(out, "\t\t. = ALIGN({});", std::mem::align_of::<W>())?;
    writeln!(out, "\t\t__end_{} = .;", section.name)?;
    writeln!(
        out,
        "\t\t. = __{}_origin + __{}_size;",
        section.vma.0, section.vma.0
    )?;
    writeln!(out, "\t\t__start_{} = .;", section.name)?;
    writeln!(out, "\t}} > {}", section.vma.0)?;
    writeln!(out, "")?;
    Ok(())
}

/// render a heap section
fn render_fixed_section<W: Word, Wr: Write>(
    out: &mut Wr,
    section: &Section<W>,
    size: W,
) -> Result<(), Error> {
    writeln!(out, "\t.{} :", section.name)?;
    writeln!(out, "\t{{")?;
    writeln!(out, "\t\t__start_{} = .;", section.name)?;
    writeln!(out, "\t\t. += {}", size)?;
    writeln!(out, "\t\t__end_{} = .;", section.name)?;
    writeln!(out, "\t}} > {}", section.vma.0)?;
    writeln!(
        out,
        "\t__{}_used = __{}_used + SIZEOF(.{});",
        section.vma.0, section.vma.0, section.name
    )?;
    writeln!(out, "")?;
    Ok(())
}

/// Generate a linker script from a LinkerScript
pub fn render<W: Word, Wr: Write>(ls: &LinkerScript<W>, out: &mut Wr) -> Result<(), Error> {
    // file header
    writeln!(
        out,
        "INCLUDE device.x
ENTRY(Reset);
EXTERN(__RESET_VECTOR); /* depends on the `Reset` symbol */

/* # Exception vectors */
/* This is effectively weak aliasing at the linker level */
/* The user can override any of these aliases by defining the corresponding symbol themselves (cf.
   the `exception!` macro) */
EXTERN(__EXCEPTIONS); /* depends on all the these PROVIDED symbols */

EXTERN(DefaultHandler);

PROVIDE(NonMaskableInt = DefaultHandler);
EXTERN(HardFaultTrampoline);
PROVIDE(MemoryManagement = DefaultHandler);
PROVIDE(BusFault = DefaultHandler);
PROVIDE(UsageFault = DefaultHandler);
PROVIDE(SecureFault = DefaultHandler);
PROVIDE(SVCall = DefaultHandler);
PROVIDE(DebugMonitor = DefaultHandler);
PROVIDE(PendSV = DefaultHandler);
PROVIDE(SysTick = DefaultHandler);

PROVIDE(DefaultHandler = DefaultHandler_);
PROVIDE(HardFault = HardFault_);

/* # Interrupt vectors */
EXTERN(__INTERRUPTS); /* `static` variable similar to `__EXCEPTIONS` */
"
    )?;

    writeln!(out, "MEMORY {{")?;
    for region in ls.regions.values() {
        writeln!(
            out,
            "\t{} : ORIGIN = {:#X}, LENGTH = {:#X}",
            region.name, region.origin, region.size
        )?;
    }
    writeln!(out, "}}")?;

    writeln!(out, "SECTIONS {{")?;
    for region in ls.regions.values() {
        writeln!(out, "\t__{}_origin = {};", region.name, region.origin)?;
        writeln!(out, "\t__{}_size = {};", region.name, region.size)?;
        writeln!(out, "\t__{}_used = 0;", region.name)?;
    }
    let mut sorted_sections: Vec<Section<W>> = ls.sections.values().cloned().collect();
    sorted_sections.sort_by(|a, b| a.priority.partial_cmp(&b.priority).unwrap());
    for section in sorted_sections.iter() {
        match section.size {
            SectionSize::Linker => render_linker_section(out, section)?,
            SectionSize::Heap => render_heap_section(out, section)?,
            SectionSize::Stack => render_stack_section(out, section)?,
            SectionSize::Fixed(size) => render_fixed_section(out, section, size)?,
        }
    }

    writeln!(out, "}}")?;

    //TODO assign a symbol describing the size of each region
    //and section. The section sizes are needed for double linking
    //when introspecting the resulting elf and rebuilding
    //The region sizes are needed in some cases for flash configuration
    //tables (ex: external flash based devices).

    Ok(())
}
