mod domain;

use epub::doc::EpubDoc;
use crate::domain::services::extract_content::extract_content;

fn main() {
    pretty_env_logger::init();
    println!("Hello, world!");

    let doc = EpubDoc::new("src/tests/minimal_sample.epub");
    assert!(doc.is_ok());
    let mut doc = doc.unwrap();

    let title = doc.mdata("title").unwrap();
    println!("Title: {}", title);

    let ressources_len = doc.resources.len();
    println!("Ressources: {}", ressources_len);

    let spin_0 = doc.spine[0].clone();
    println!("Spin 0: {:?}", spin_0);

    // Current chapter number
    let cur_page = doc.get_current_page();
    println!("Current page: {:?}", cur_page);

    doc.go_next();
    println!("Current page: {:?}", doc.get_current_page());

    let cur_content = doc.get_current_str();
    println!("Current content: {:?}", cur_content);

    // Number of chapters
    let nb_pages = doc.get_num_pages();
    println!("Number of pages: {:?}", nb_pages);

    doc.set_current_page(13);
    let cur_content = doc.get_current_str();
    println!("Current page: {:?}", doc.get_current_page());
    println!("Current content: {:?}", cur_content);
}
