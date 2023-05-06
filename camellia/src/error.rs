use thiserror::Error;

#[derive(Error, Debug)]
pub enum CamelliaError {
    #[error("system error, {0}")]
    SystemError(#[from] nix::errno::Errno),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}
