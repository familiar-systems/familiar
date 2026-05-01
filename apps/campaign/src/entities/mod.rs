pub mod blocks;
pub mod campaign_metadata;
pub mod columns;
pub mod things;

// NOTE: There's no `users` entity here yet. We do need one eventually -
// the campaign DB needs local user records to answer authz questions
// (who can read this campaign, who can delete it, who can kick off audio
// processing) without round-tripping to apps/platform for every check.
// Campaign-pinned actor isolation only works if the questions live with
// the data. The shape of the table (mirror of platform.users? local
// handles only? just IDs with cached display names?) is a design
// decision deferred to when per-campaign permissions land.
