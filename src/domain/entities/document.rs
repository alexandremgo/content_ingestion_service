/// A searchable section of a book
/// 
/// * section_id is the incremental number representing
///   the order of the section in the book
pub struct Document {
    pub id: String,
    pub book_id: String,
    pub section_id: u32,
    pub content: String,
}

pub struct Book {
    pub book_id: String,
    pub author: String,
}
