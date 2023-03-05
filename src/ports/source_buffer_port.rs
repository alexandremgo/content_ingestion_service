use crate::domain::entities::source::SourceChar;

#[derive(Debug)]
pub enum NextError {
    Unknown,
}

pub trait SourceBufferPort {
  fn next(&mut self) -> Result<Option<SourceChar>, NextError>;
}
