use crate::domain::entities::source::SourceChar;

pub enum CreateError {
    Unknown,
    SourceNotFound,
}

pub enum NextError {
    Unknown,
    Ended,
}

pub trait SourceBufferPort {
  fn create(source_file_path: String) -> Result<Box<Self>, CreateError>;
  fn next(&self) -> Result<SourceChar, NextError>;
}
