use actix_multipart::form::{MultipartForm, tempfile::TempFile};
use actix_web::HttpResponse;

#[derive(Debug, MultipartForm)]
pub struct UploadForm {
    #[multipart(rename = "file")]
    files: Vec<TempFile>,
}

/// File struct:
/// An object providing access to an open file on the filesystem.
///
/// An instance of a `File` can be read and/or written depending on what options
/// it was opened with. Files also implement [`Seek`] to alter the logical cursor
/// that the file contains internally.
///
/// Files are automatically closed when they go out of scope.  Errors detected
/// on closing are ignored by the implementation of `Drop`.  Use the method
/// [`sync_all`] if these errors must be manually handled.
pub async fn add_source_files(MultipartForm(form): MultipartForm<UploadForm>) -> HttpResponse {
    // TODO: real user
    let user_id = "test_user";

    for f in form.files {
        
        // let path = format!("./tmp/{}", f.file_name.unwrap());
        // log::info!("saving to {path}");
        // f.file.persist(path).unwrap();
    }

    HttpResponse::Ok().finish()
}

// Could be a repository port/interface
fn save_file_in_bucket(bucket_name: String) -> Result<(), ()> {
    Ok(())
}

fn check_or_create_bucket(bucket_name: String) -> Result<(), ()> {
    Ok(())
}
