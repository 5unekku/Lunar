# libGDX, Pygame, LÖVE2D — Minimal 2D API Research

## libGDX (Java)

### Key Concepts

**SpriteBatch**
- Batches sprite draw calls into a single OpenGL call
- Begin/end pattern: `batch.begin()` → draw calls → `batch.end()`
- Automatic texture switching when different regions used
- Camera matrix applied to all sprites

**TextureAtlas**
- Packs multiple textures into single image + metadata file
- Reduces texture switches during rendering
- `TextureRegion` references a sub-rectangle of the atlas
- Atlas loading is async-capable

**BitmapFont**
- Pre-rendered font textures with glyph metrics
- Supports multiple pages for large character sets
- Markup language for colors/styles inline: `[RED]text[]`
- Cache text layout for reuse

**Stage/Actor UI**
- `Stage` manages a scene graph of `Actor` objects
- Actors have position, size, rotation, origin, scale
- Input multiplexer routes events to focused actor
- `Table` layout system for complex UI arrangements

### Lessons for Lunar

1. **SpriteBatch pattern** — Lunar's RenderEngine already batches, but the begin/end pattern is cleaner than command queues for simple cases.

2. **TextureAtlas** — Lunar's glyph atlas is a start. Full texture atlas support would reduce texture switches:
   ```yaml
   atlas:
     image: "sprites.png"
     regions:
       player: [0, 0, 32, 32]
       enemy: [32, 0, 32, 32]
       bullet: [64, 0, 8, 8]
   ```

3. **Table layout** — libGDX's Table is simpler than flexbox but powerful for game UI:
   ```
   table.add(label).expandX().fillX()
   table.row()
   table.add(button).width(200).padTop(10)
   ```

---

## Pygame (Python)

### Key Concepts

**Immediate Mode**
- No scene graph — draw directly to surface each frame
- `screen.blit(image, position)` for each sprite
- Manual dirty rectangle tracking for optimization
- Simple but requires manual management

**Surface System**
- `Surface` is the basic drawable (image, text, shape)
- `Rect` for position/size with collision helpers
- Alpha blending, color keys, transforms built-in
- Sub-surfaces share pixel data with parent

**Event Queue**
- `pygame.event.get()` returns all pending events
- Event types: KEYDOWN, KEYUP, MOUSEMOTION, MOUSEBUTTONDOWN, etc.
- Custom event types via `pygame.event.Event()`
- Event filtering by type

### Lessons for Lunar

1. **Immediate mode option** — Pygame's simplicity is appealing. Lunar could offer an immediate mode API for simple games:
   ```rust
   render.draw_immediate(|draw| {
       draw.sprite(&player_texture, pos);
       draw.rect(&rect, color);
       draw.text(&font, "Score: 100", text_pos);
   });
   ```

2. **Rect helpers** — Pygame's Rect has `collidepoint()`, `colliderect()`, `inflate()`, `clamp()`. Lunar's Rect type could add these.

3. **Event filtering** — Simple event type filtering is cleaner than complex input mappings for some cases.

---

## LÖVE2D (Lua)

### Key Concepts

**Simple API**
- `love.draw()` called each frame — draw everything here
- `love.update(dt)` for game logic
- `love.load()` for initialization
- `love.keypressed()`, `love.mousepressed()` for input

**Drawing**
- `love.graphics.draw(image, x, y, rotation, scale)`
- `love.graphics.print(text, x, y)`
- `love.graphics.rectangle(mode, x, y, w, h)`
- `love.graphics.setColor(r, g, b, a)` — state-based coloring

**Scene Management**
- No built-in scene system — use libraries or roll your own
- Common pattern: scene table with `enter()`, `update()`, `draw()`, `exit()`
- Scene manager pushes/pops scenes

**UI Libraries**
- No built-in UI — community libraries fill the gap
- `loveframes`, `imperative`, `nuklear-lua` are popular
- Immediate mode (nuklear) vs retained mode (loveframes)

### Lessons for Lunar

1. **Simple callback API** — LÖVE's `love.draw()` pattern is intuitive. Lunar's render system could expose a similar callback:
   ```rust
   app.on_render(|render, time| {
       // draw everything here
   });
   ```

2. **Scene pattern** — LÖVE's scene pattern (enter/update/draw/exit) is clean and decoupled. Lunar's SceneManager already has this.

3. **Community UI libraries** — LÖVE doesn't ship with UI, letting the community build better solutions. Lunar could do the same — provide primitives but let the community build UI frameworks on top.

---

## Cross-Engine Patterns

### Common Themes

1. **Batching is universal** — Every engine batches draw calls for performance
2. **Texture atlases reduce switches** — Packing textures is standard practice
3. **Layout systems vary** — From manual (Pygame) to flexbox (Bevy) to Table (libGDX)
4. **Event systems decouple** — Signals, events, callbacks all serve the same purpose
5. **Scene management is essential** — Every engine has some form of scene switching

### Lunar-Specific Recommendations

1. **Add TextureAtlas support** — Already have glyph atlas, extend to general use
2. **Add Rect collision helpers** — `contains()`, `intersects()` exist, add `inflate()`, `clamp()`
3. **Consider taffy for layout** — Pure Rust flexbox, WASM compatible
4. **Keep UI decoupled** — UI produces DrawCommands, doesn't own rendering
5. **Offer multiple UI paradigms** — Retained mode (components) + immediate mode (callbacks)
