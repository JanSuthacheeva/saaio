//! `saaio --stop`: stops the server and every session, refused on a worker's own store

use crate::cli::Env;
use crate::cli::context::{Context, Opened};
use crate::cli::output::{Document, Failure};
use crate::client;
use crate::id::FlowId;
use crate::store::Store;

/// Stops the server of the store and every session in it, one saaio 0.1.0 started too; refused on a
/// worker's own store.
pub fn run(ctx: &Context) -> Result<Document, Failure> {
    let Opened { store, .. } = ctx.open()?;
    refuse_inside_own_flow(&store, ctx.env)?;
    let socket = ctx.env.socket(&store);
    let mut stopped = client::stop_server(&socket, ctx.clock)?;
    if let Some(earlier) = client::earlier_socket(&store).filter(|earlier| *earlier != socket) {
        // Where saaio 0.1.0 put a long store's socket; one there that cannot be stopped is no
        // server of this store's.
        stopped |= client::stop_server(&earlier, ctx.clock).unwrap_or(false);
    }
    let message = if stopped {
        "stopped the saaio server and every session in it"
    } else {
        "no saaio server runs"
    };
    Ok(Document::Text(message.to_string()))
}

/// Err when $SAAIO_FLOW_ID names a flow in `store`: a worker's stop would end every session of the
/// user's, its own among them.
fn refuse_inside_own_flow(store: &Store, env: &Env) -> Result<(), Failure> {
    let Some(id) = env.flow_id().and_then(|text| FlowId::parse(text).ok()) else {
        return Ok(());
    };
    if !store.ids()?.contains(&id) {
        return Ok(());
    }
    Err(Failure::error(
        "`saaio --stop` runs inside a flow's worker, on the store that flow lives in: it would end the user's sessions",
        vec!["Set SAAIO_HOME to a scratch folder to stop a server of your own".to_string()],
    ))
}
