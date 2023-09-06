use actix_web::{
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    error::ErrorUnauthorized,
    http, web, HttpMessage,
};
use futures::{future::LocalBoxFuture, FutureExt};
use serde::{Deserialize, Serialize};
use std::{
    future::{ready, Ready},
    rc::Rc,
    task::{Context, Poll},
};
use tracing::error;
use uuid::Uuid;

use crate::repositories::jwt_authentication_repository::JwtAuthenticationRepository;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserIdFromToken(pub Uuid);

/// Middleware responsible for handling authentication and user information extraction.
pub struct AuthMiddleware<S> {
    service: Rc<S>,
    auth_repository: web::Data<JwtAuthenticationRepository>,
}

impl<S> Service<ServiceRequest> for AuthMiddleware<S>
where
    S: Service<
            ServiceRequest,
            Response = ServiceResponse<actix_web::body::BoxBody>,
            Error = actix_web::Error,
        > + 'static,
{
    type Response = ServiceResponse<actix_web::body::BoxBody>;
    type Error = actix_web::Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, actix_web::Error>>;

    /// Polls the readiness of the wrapped service.
    fn poll_ready(&self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    /// Handles incoming requests.
    fn call(&self, req: ServiceRequest) -> Self::Future {
        // Attempt to extract token from authorization header only
        let token = req
            .headers()
            .get(http::header::AUTHORIZATION)
            .map(|h| h.to_str().unwrap().split_at(7).1.to_string());

        // If token is missing, return unauthorized error
        let token = match token {
            Some(token) => token,
            None => {
                return Box::pin(ready(Err(ErrorUnauthorized(
                    "No access token was provided",
                ))));
            }
        };

        // Decode token and handle errors
        let user_id = match self.auth_repository.decode_token(&token) {
            Ok(id) => id,
            Err(e) => return Box::pin(ready(Err(e.into()))),
        };

        let user_id = match Uuid::parse_str(user_id.as_str()) {
            Ok(user_id) => user_id,
            Err(error) => {
                error!(?error, "Provided user id could not be parsed to uuid");

                return Box::pin(ready(Err(ErrorUnauthorized(
                    "Provided user id is not valid",
                ))));
            }
        };

        let srv = Rc::clone(&self.service);

        // Handles user id extraction, insertion into request extensions and continue the request processing
        async move {
            req.extensions_mut()
                .insert::<UserIdFromToken>(UserIdFromToken(user_id));

            // Calls the wrapped service to handle the request
            let res = srv.call(req).await?;
            Ok(res)
        }
        .boxed_local()
    }
}

/// Middleware factory for requiring authentication.
pub struct RequireAuth {
    auth_repository: web::Data<JwtAuthenticationRepository>,
}

impl RequireAuth {
    pub fn new(auth_repository: web::Data<JwtAuthenticationRepository>) -> Self {
        Self { auth_repository }
    }
}

impl<S> Transform<S, ServiceRequest> for RequireAuth
where
    S: Service<
            ServiceRequest,
            Response = ServiceResponse<actix_web::body::BoxBody>,
            Error = actix_web::Error,
        > + 'static,
{
    type Response = ServiceResponse<actix_web::body::BoxBody>;
    type Error = actix_web::Error;
    type Transform = AuthMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    /// Creates and returns a new AuthMiddleware wrapped in a Result.
    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AuthMiddleware {
            service: Rc::new(service),
            auth_repository: self.auth_repository.clone(),
        }))
    }
}
