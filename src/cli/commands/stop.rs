use crate::cli::Env;
use crate::cli::output::{Document, Failure};
use crate::client;
use crate::store::Store;

/// Stops the server of `store` and every session in it; refused on a worker's own store.
pub fn run(store: &Store, env: &Env, clock: fn() -> u64) -> Result<Document, Failure> {
    refuse_inside_own_flow(store, env)?;
    let message = if client::stop_server(&client::socket_path(store), clock)? {
        "stopped the saaio server and every session in it"
    } else {
        "no saaio server runs"
    };
    Ok(Document::Text(message.to_string()))
}

/// Err when $SAAIO_FLOW_ID names a flow in `store`: a worker's stop would end every session of the
/// user's, its own among them.
fn refuse_inside_own_flow(store: &Store, env: &Env) -> Result<(), Failure> {
    let Some(id) = env.flow_id() else {
        return Ok(());
    };
    if !store.ids()?.iter().any(|known| known == id) {
        return Ok(());
    }
    Err(Failure::error(
        "`saaio --stop` runs inside a flow's worker, on the store that flow lives in: it would end the user's sessions",
        vec!["Set SAAIO_HOME to a scratch folder to stop a server of your own".to_string()],
    ))
}
