//! Convenience flags build the same strict shared import request as JSON.
use kronello_service::{
    BackendSelection, ProjectRequest, Request, ResultData, Service, ServiceError,
};
use serde_json::{Map, Value};

pub fn payload(args: &[&str], selection: BackendSelection) -> Result<String, ServiceError> {
    let mut fields = Map::new();
    for pair in args.chunks(2) {
        let [flag, value] = pair else {
            return Err(ServiceError::invalid(
                "media import option requires a value",
            ));
        };
        let key = match *flag {
            "--project" => "project",
            "--asset" => "asset",
            "--path" => "path",
            "--base-revision" => "base_revision",
            "--session-id" => "session_id",
            "--idempotency-key" => "idempotency_key",
            _ => {
                return Err(ServiceError::invalid(format!(
                    "unknown media import option: {flag}"
                )));
            }
        };
        if fields
            .insert(key.into(), Value::String((*value).into()))
            .is_some()
        {
            return Err(ServiceError::invalid(format!("duplicate option: {flag}")));
        }
    }
    for key in ["project", "asset", "path"] {
        if !fields.contains_key(key) {
            return Err(ServiceError::invalid(format!(
                "media import requires --{key}"
            )));
        }
    }
    if !fields.contains_key("base_revision") {
        let ResultData::Project(info) =
            Service::new(selection).dispatch(Request::ProjectInfo(ProjectRequest {
                project: fields["project"].as_str().unwrap().into(),
            }))?
        else {
            unreachable!("project.info result")
        };
        fields.insert("base_revision".into(), Value::String(info.revision));
    }
    for key in ["session_id", "idempotency_key"] {
        fields
            .entry(key)
            .or_insert_with(|| Value::String(uuid::Uuid::new_v4().to_string()));
    }
    Ok(serde_json::to_string(&fields)?)
}
