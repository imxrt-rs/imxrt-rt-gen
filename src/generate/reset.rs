use crate::{LinkerScript, Word};
use std::io::{Error, Write};

/// Generate a reset module from a LinkerScript
pub fn render<W: Word, Wr: Write>(out: &mut Wr,
                                  linker_script: &LinkerScript<W>
) -> Result<(), Error> {
    writeln!(out, "#[doc(Hidden)]");
    writeln!(out, "#[link_section = \".vector_table.reset_vector\"]");
    writeln!(out, "#[no_mangle]");
    writeln!(out, "pub static __RESET_VECTOR: unsafe extern \"C\" fn() -> ! = Reset;");
    Ok(())
}
