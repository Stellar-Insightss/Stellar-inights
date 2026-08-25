use crate::Error;

pub fn validate_transition(_source: u32, target: u32) -> Result<(), Error> {
    // Version zero is reserved for legacy storage without a marker. Explicit
    // forward and rollback/repair transitions are both governance-controlled.
    if target == 0 {
        return Err(Error::InvalidSchemaTransition);
    }
    Ok(())
}
