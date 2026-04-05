/// Campaign content is filtered at the retrieval layer based on the users' permission.
/// See: docs/plans/2026-02-22-ai-prd.md
pub enum Status {
    /// This is known only to the GM.
    /// It could be a secret plot point or hidden story arc.
    /// Or it could be some piece of lore or background that the GM hasn't decided on yet.
    /// Regardless, only the GM is aware of it but AI treats it as fact.
    GmOnly,
    /// This is known to players.
    /// It has either been revealed through play or the GM has explicitly shared it.
    Known,
    /// This was canon but has been retconned during play.
    Retconned,
}
