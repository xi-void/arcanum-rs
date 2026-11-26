# Arcanum-RS — Manual Regression Checklist

## Startup and rendering
- [ ] Game launches at Crash Site.
- [ ] World renders without missing textures or obvious corruption.
- [ ] Animations, lighting and shadows work.
- [ ] Window resizing works.

## Movement and camera
- [ ] Mouse-driven walking and running.
- [ ] Pathfinding around obstacles.
- [ ] World zoom in both directions.
- [ ] Camera movement and recentering.
- [ ] Door opening and map transitions.

## HUD and interface
- [ ] HUD buttons respond to hover and clicks.
- [ ] UI scaling works with `[` and `]`.
- [ ] Opening panels does not resize the HUD.
- [ ] No overlapping interface regions.
- [ ] Rendering and pointer hitboxes remain aligned.
- [ ] Contextual descriptions appear in the bottom HUD.
- [ ] Panels toggle correctly and close with Escape.

## Maps and world travel
- [ ] Local map displays correctly.
- [ ] World map displays correctly.
- [ ] Map panning and zooming.
- [ ] Location hover descriptions.
- [ ] Route selection and world travel.
- [ ] Underground travel restrictions.

## Inventory and containers
- [ ] Inventory opens and closes through both inputs.
- [ ] Original container types render and open.
- [ ] Locked containers reject ordinary access.
- [ ] Multi-cell items display correctly.
- [ ] Item hover descriptions and highlights.
- [ ] Container scrolling.
- [ ] Inventory and HUD scaling remain synchronized.

## Developer functionality
- [ ] Developer console opens in debug mode.
- [ ] Teleportation and collision bypass.
- [ ] RPC responds to documented commands.

## Performance
- [ ] No visible movement stuttering.
- [ ] No noticeable UI scaling delays.
- [ ] No significant performance regression during exploration.

## Before merging
- [ ] Relevant automated tests pass.
- [ ] Formatting and strict Clippy pass.
- [ ] GitHub CI passes.
- [ ] Outstanding defects are documented.
