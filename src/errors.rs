use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Seed range goes out of bounds")]
    SeedRangeOutOfBounds,

    #[error("Can't communicate with child")]
    BrokenChildCommunication,
}