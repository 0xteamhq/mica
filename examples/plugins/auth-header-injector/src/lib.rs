// Sample plugin (P5.3): injects X-Tenant-ID derived from the inbound
// Authorization header onto every upstream request.
//
// Build: cd examples/plugins/auth-header-injector && cargo component build --release

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
    fn on_file_created(_info: FileInfo) -> Result<String, String> {
        Ok(String::new())
    }
}

impl Http for Component {
    fn intercept_request(mut req: Request) -> Result<Request, String> {
        // Look for "Authorization: Bearer <token>" and drop a
        // tenant id derived from the first 8 chars of the token.
        let auth = req
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            .map(|(_, v)| v.clone());
        if let Some(v) = auth {
            let token = v.trim_start_matches("Bearer ").trim();
            if !token.is_empty() {
                let tenant: String = token.chars().take(8).collect();
                req.headers.push(("x-tenant-id".to_string(), tenant));
            }
        }
        Ok(req)
    }
    fn intercept_response(resp: Response) -> Result<Response, String> {
        Ok(resp)
    }
}

bindings::export!(Component with_types_in bindings);

// Silence the unused-import warning when the binding macro is the
// only consumer of these symbols.
#[allow(dead_code)]
fn _kind_unused() -> ArtifactKind {
    ArtifactKind::Log
}
