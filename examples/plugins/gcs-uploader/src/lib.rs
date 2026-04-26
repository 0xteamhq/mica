// Sample plugin (P5.3): GCS uploader for FileCreated events.
//
// The plugin's wasm component declares an `outgoing-handler`
// import (WASI HTTP) that mica grants. The implementation here
// is a stub — it logs the path and returns the GCS object URL
// it would have uploaded to. Real GCS auth (service-account JSON
// + signed URLs) lives in a follow-up commit once mica's plugin
// capability-grant story lands.

#[allow(warnings)]
mod bindings {
    wit_bindgen::generate!({
        world: "mica-plugin",
        path: "../../../wit",
    });
}

use bindings::exports::mica::plugin::artifact::{ArtifactKind, FileInfo, Guest as Artifact};
use bindings::exports::mica::plugin::http::{Guest as Http, Request, Response};
use bindings::exports::mica::plugin::lifecycle::Guest as Lifecycle;
use bindings::exports::mica::plugin::session::{Guest as Session, SessionInfo};

struct Component;

const BUCKET: &str = "mica-artifacts";

impl Lifecycle for Component {
    fn init() -> Result<(), String> {
        Ok(())
    }
    fn shutdown() {}
}

impl Session for Component {
    fn on_create(_info: SessionInfo) -> Result<String, String> {
        Ok(String::new())
    }
    fn on_end(_session_id: String, _started: u64, _finished: u64) {}
}

impl Artifact for Component {
    fn on_file_created(info: FileInfo) -> Result<String, String> {
        let prefix = match info.kind {
            ArtifactKind::Video => "video",
            ArtifactKind::Log => "log",
        };
        let key = format!(
            "{prefix}/{session}/{name}",
            session = info.session_id,
            name = info
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&info.session_id),
        );
        Ok(format!("gs://{BUCKET}/{key}"))
    }
}

impl Http for Component {
    fn intercept_request(req: Request) -> Result<Request, String> {
        Ok(req)
    }
    fn intercept_response(resp: Response) -> Result<Response, String> {
        Ok(resp)
    }
}

bindings::export!(Component with_types_in bindings);
