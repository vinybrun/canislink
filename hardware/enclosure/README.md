# Enclosure 3D models

OpenSCAD sources for the CanisLink terminal.

| File | Description |
|------|-------------|
| `terminal_base.scad` | Presence mat + console assembly (print in sections) |
| `button_pad.scad` | 100 mm convex textured pad |

## Render STL

```bash
# requires openscad
openscad -o terminal_base.stl terminal_base.scad
openscad -o button_pad.stl button_pad.scad
```

If OpenSCAD is not installed, CI still validates the `.scad` files exist and are non-empty; local lab prints use the STL export step.

## Dog-safety notes

- No sharp edges under 3 mm fillet in production mold
- Non-slip mat base
- Washable silicone pad covers
- Console cable strain relief away from chew zone
