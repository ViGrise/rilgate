use axum::{extract::Request, http::HeaderName};
use tower_http::request_id::{
  MakeRequestId, PropagateRequestIdLayer, RequestId, SetRequestIdLayer,
};

#[derive(Clone, Default)]
pub struct Id;

impl MakeRequestId for Id {
  fn make_request_id<B>(&mut self, _: &Request<B>) -> Option<RequestId> {
    let id = uuid::Uuid::now_v7().to_string().parse().ok()?;
    Some(RequestId::new(id))
  }
}

pub fn request_id_layer() -> SetRequestIdLayer<Id> {
  let x_request_id = HeaderName::from_static("x-request-id");
  SetRequestIdLayer::new(x_request_id, Id)
}

pub fn propagate_request_id_layer() -> PropagateRequestIdLayer {
  let x_request_id = HeaderName::from_static("x-request-id");
  PropagateRequestIdLayer::new(x_request_id)
}
