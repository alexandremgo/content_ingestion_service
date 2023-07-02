/// Meta information (chapter) about the current read index
///
/// TODO: could have an incremental id so the extractor can know when to stop the current document ?
pub trait MetaRead {
    fn current_read_meta(&self) -> Option<String>;
}
