use thiserror::Error;
#[derive(Error, Debug)]
#[error("Proof-of-work capacity is exhausted")]
pub struct CapacityExceeded;

#[derive(Error, Debug)]
#[error("Proof-of-work backend is unavailable")]
pub struct PowBackendUnavailable;
