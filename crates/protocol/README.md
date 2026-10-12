# Catalog-bound item protocol

Protocol **v6** uses a local generated catalog, not an embedded item list.
`BindCatalog` identifies an installation and immutable catalog generation using
two SHA-256 identifiers. The adapter loads and verifies that metadata on its IPC
worker before binding it to the local game session. The complete catalog never
crosses the 16 KiB pipe.

`InspectItem` and typed `Spawn` requests carry the same catalog identity and an
item's logical path. Responses identify the bound generation. Mismatched,
unbound, stale, or unknown entries cannot authorize a spawn. Equipment,
collectible, and world-pickup options retain separate wire shapes. Unknown
categories and records without supported initialization reject every option family.

A complete, syntactically valid current-version bind/inspect/spawn request has
a 30-second response budget for source validation and package loading. Framing
still has a two-second deadline; incomplete messages do not extend it. Catalog
membership and runtime support are checked after framing. Old versions reject.

Confirmed outcomes describe floor actors, not inventory acquisition. Failed
batches retain confirmed counts and uncertainty; ambiguous actions never retry.

See [catalog implementation](../catalog/README.md) and the
[validation report](../../docs/automatic-catalog.md).
