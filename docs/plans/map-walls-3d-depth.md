# Map — Solid Walls 3D Depth Polish

**Board card:** Map: solid walls taller, richer material, deeper shadows (ToDo)
**Goal:** Three targeted Three.js tweaks that give solid walls convincing architectural mass — taller extrusion, surface feel, and shadow contrast — with no data-model or backend change.

## Phase 1 — Taller solid walls

**File:** `sentinel-app/src/lib/three/map/WallSegment.svelte` · line 24

Current formula: `Math.max(0.05, wall.thickness * 2)`
Change to: `Math.max(0.2, wall.thickness * 4)`

- 2× the height multiplier; minimum raised to 0.2 so even the thinnest solid wall reads as a wall, not a slab.
- Wireframe style (`height = 0.04`) unchanged — it's a flat footprint outline, no height needed.
- The endpoint-handle sphere offset (`height + 0.12`) follows automatically since it uses the derived `height`.

## Phase 2 — Richer material feel

**File:** `sentinel-app/src/lib/three/map/WallSegment.svelte` · line 39

Current: `<T.MeshStandardMaterial color={fillColor} roughness={0.8} />`
Change to: `<T.MeshStandardMaterial color={fillColor} roughness={0.65} metalness={0.06} />`

- `roughness` down from 0.8 → 0.65: painted-plaster/concrete feel — micro-specularity without looking metallic.
- `metalness={0.06}`: enough for the key light to leave a faint highlight across the wall face; imperceptible in isolation but adds depth when lit from the side.
- `INK_FILL` (`#cdd6dc`) stays as-is — the cooler blue-grey reads like a clean architectural model.

## Phase 3 — Deeper shadows

**File:** `sentinel-app/src/lib/three/map/MapScene.svelte` · lines 167–174

Changes:
- `AmbientLight intensity`: `0.85` → `0.5` — ambient is the main shadow-killer; halving it reveals the key-light shadow without darkening the floor (ground receives direct light).
- `DirectionalLight intensity`: `1.15` → `1.7` — compensates the ambient drop; total scene brightness stays similar, but now it's directional, so walls cast real shadow.
- `shadow.mapSize`: `2048` → `4096` on both width and height — eliminates the visible staircase on diagonal wall shadows at normal zoom levels.
- Shadow camera frustum: tighten from ±120 → ±80 — smaller frustum means higher shadow-texel density for the typical map area; if maps grow this can be widened back.

```svelte
<T.AmbientLight intensity={0.5} />
<T.DirectionalLight
  position={[40, 70, 24]}
  intensity={1.7}
  castShadow
  shadow.mapSize.width={4096}
  shadow.mapSize.height={4096}
  shadow.camera.left={-80}
  shadow.camera.right={80}
  shadow.camera.top={80}
  shadow.camera.bottom={-80}
  shadow.camera.near={1}
  shadow.camera.far={400}
/>
```

The cool fill light (`[-30, 18, -40]`, intensity 0.3) stays unchanged — it reads as a secondary sky-bounce that keeps the plan feeling paper-white, not cave-dark.

## Out of scope (v1)

- Normal/bump maps on walls (would require texture loading)
- Per-wall material variants (concrete vs plaster vs glass)
- Point-light sconces or ambient occlusion baking
- Shadow-casting for markers or room floors
