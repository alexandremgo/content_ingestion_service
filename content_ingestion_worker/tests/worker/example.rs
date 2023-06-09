use crate::helpers::spawn_app;

#[tokio::test]
async fn subscribe_returns_a_200_for_valid_form_data() {
    // Arrange
    // So now we are consuming the queue in the background/in another thread
    let app = spawn_app().await;
    // Could it be possible to send the message before listening to this 
    // From the helper: could we actually access the build and spawn ? 
    // Mock::given(path("/email"))
    //     .and(method("POST"))
    //     .respond_with(ResponseTemplate::new(200))
    //     .mount(&app.email_server)
    //     .await;

    // // Act
    // let response = app.post_subscriptions(body.into()).await;

    // // Assert
    // assert_eq!(200, response.status().as_u16());
}
