/// Tells tonic-build to compile your protobufs when you build your Rust project.
/// While you can configure this build process in a number of ways
/// we will not get into the details in this introductory tutorial.
/// Please see the tonic-build documentation for details on configuration.
fn main() {
  tonic_build::compile_protos("proto/fulltext_search_service.proto")
      .unwrap_or_else(|e| panic!("Failed to compile protos {:?}", e));
}
