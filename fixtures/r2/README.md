# R2 production replay fixtures

These are complete CollectedPayload records acquired on 2026-09-09 through this checkout's target/debug/devup-mcp.exe using the original WQUW-118/119/120 arguments, then rawPayload requested from that local server's artifact. They are executable source captures, not reconstructed nodes from generated code.

- WQUW-119: six frames, 227 nodes, original contentHash 38aa5a2665ee86b845a0d503989060b0c68a1a3946353ca4154cb24f653d7670 reproduced exactly. This fixture contains the visible, opaque but clipped 3997:46703 image.
- WQUW-118: three frames, 208 nodes, includes the auto-sizing Text's two uncovered dimensions.
- WQUW-120: one frame, 57 nodes, generates inline TSX and component references with 40 uncovered fields in five component usages.

Tests in server/projection.rs run without Figma credentials or network calls. Original live responses and collection call cache are kept outside target in C:/Users/owjs3/orca/devup-mcp-briefs.
