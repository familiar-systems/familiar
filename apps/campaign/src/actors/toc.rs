use crate::actors::database_writer;
use crate::loro::toc::LoroTocDoc;

// --- Actor ---

/// The TocActor coordinates the campaign's table of contents.
///
/// Under the hood, it uses the [`LoroTocDoc`] to store the
/// table of contents.
///
/// For agents, it backs the `ls` command.
///
/// TODO: Suggestion hydration on checkout. The restore path should:
/// 1. Rebuild the LoroTree from SQLite with structural entries (Folder/Thing).
///    This gives clients a fast first paint.
/// 2. Query SQLite for pending suggestions on this campaign's ToC.
/// 3. Apply them as CRDT updates: inline suggestions (change/delete) go into
///    node metadata; new-entry suggestions become `kind: "suggestion"` nodes.
/// 4. These updates stream to clients via loro-protocol sync.
///
/// During the active session, the CRDT is authoritative. Suggestions arrive
/// via actor messages (from AgentConversation actors) and are applied directly
/// to the LoroDoc. The debounce timer writes everything back to SQLite.
/// On eviction, a final snapshot persists to SQLite and the doc is dropped.
#[allow(dead_code)]
pub struct TocActor {
    doc: LoroTocDoc,
    db_writer: database_writer::DatabaseActor,
    ///If dirty, this has yet to be synced back to the database.
    dirty: bool,
}
