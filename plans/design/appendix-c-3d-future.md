# 3D Future Compatibility

The current 2D architecture is designed to not block 3D:

- `Transform` uses `Vec3` position, not `Vec2`
- `RenderQueue` is a command buffer, not a direct 2D API — 3D commands can be added
- `Camera` has rotation (unused in 2D, needed for 3D)
- `engine-math` already re-exports `Vec3`, `Vec4`, `Mat3`, `Mat4`
- wgpu is inherently 3D — 2D is just orthographic projection

Future 3D would add:
- `Mesh` component with vertex/index buffers
- `Material` component
- `Light` component
- 3D render pass alongside 2D

---

[← Back to Web Targets](appendix-b-web-targets.md)
