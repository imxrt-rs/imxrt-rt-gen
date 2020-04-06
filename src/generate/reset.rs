use crate::{LinkerScript, Word};
use std::io::Error;

/// Generate a reset module from a LinkerScript
pub fn render<W: Word>(_ls: &LinkerScript<W>) -> Result<Vec<u8>, Error> {
    Ok(Vec::new())
}
