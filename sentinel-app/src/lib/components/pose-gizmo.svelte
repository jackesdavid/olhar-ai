<script lang="ts">
  // Head pose as a shape you can read at a glance (#126). Yaw, pitch and roll are three signed
  // numbers in degrees, and three numbers do not tell you at a glance that a face was looking away
  // — a box turned the same way does.
  //
  // Drawn as an orthographic wireframe rotated by the three angles, plus a nose ray so the facing
  // direction stays obvious even when the box is near square-on. SVG rather than canvas or WebGL:
  // it is a dozen lines at one size, it stays crisp on any display, and it inherits the theme's
  // colours instead of re-declaring them.
  //
  // The rotation is written out rather than pulled from a matrix library — it is one 3×3 product,
  // and a dependency for that would be larger than the maths.
  interface Props {
    yaw: number;
    pitch: number;
    roll: number;
    size?: number;
  }
  const { yaw, pitch, roll, size = 96 }: Props = $props();

  const rad = (d: number) => (d * Math.PI) / 180;

  /** Rotate a point by yaw (around Y), pitch (around X), then roll (around Z). */
  function rotate([x, y, z]: [number, number, number]): [number, number, number] {
    const [cy, sy] = [Math.cos(rad(yaw)), Math.sin(rad(yaw))];
    const [cp, sp] = [Math.cos(rad(pitch)), Math.sin(rad(pitch))];
    const [cr, sr] = [Math.cos(rad(roll)), Math.sin(rad(roll))];
    // yaw
    let [X, Y, Z] = [x * cy + z * sy, y, -x * sy + z * cy];
    // pitch
    [X, Y, Z] = [X, Y * cp - Z * sp, Y * sp + Z * cp];
    // roll
    return [X * cr - Y * sr, X * sr + Y * cr, Z];
  }

  /** Orthographic projection to SVG coords. Y is flipped: SVG grows downward, the model does not. */
  function project(p: [number, number, number]): [number, number] {
    const [x, y] = rotate(p);
    const half = size / 2;
    const scale = size * 0.26;
    return [half + x * scale, half - y * scale];
  }

  // A head-ish box: wider than deep, taller than wide.
  const W = 1.0;
  const H = 1.25;
  const D = 0.9;
  const CORNERS: [number, number, number][] = [
    [-W, -H, -D], [W, -H, -D], [W, H, -D], [-W, H, -D],
    [-W, -H, D], [W, -H, D], [W, H, D], [-W, H, D],
  ];
  // back face, front face, then the four struts joining them
  const EDGES: [number, number][] = [
    [0, 1], [1, 2], [2, 3], [3, 0],
    [4, 5], [5, 6], [6, 7], [7, 4],
    [0, 4], [1, 5], [2, 6], [3, 7],
  ];

  const pts = $derived(CORNERS.map(project));
  // The nose: from the centre out through the FRONT face (+Z), so it points where the face looks.
  const noseBase = $derived(project([0, 0, 0]));
  const noseTip = $derived(project([0, 0, D * 2.0]));
  // Depth of each edge's midpoint, for the near/far fade that makes the box read as 3D at all.
  const depth = $derived(
    EDGES.map(([a, b]) => (rotate(CORNERS[a])[2] + rotate(CORNERS[b])[2]) / 2),
  );
  const far = $derived(Math.min(...depth));
  const near = $derived(Math.max(...depth));
  /** 0.25 (farthest) … 1 (nearest) — a cheap stand-in for hidden-line removal. */
  function edgeOpacity(i: number): number {
    const span = near - far || 1;
    return 0.25 + 0.75 * ((depth[i] - far) / span);
  }
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 {size} {size}"
  role="img"
  aria-label="yaw {Math.round(yaw)}°, pitch {Math.round(pitch)}°, roll {Math.round(roll)}°"
  class="shrink-0"
>
  <!-- Ground reference, so a rotation is measured against something rather than floating. -->
  <line
    x1={size * 0.1} y1={size / 2} x2={size * 0.9} y2={size / 2}
    class="stroke-border" stroke-width="1" stroke-dasharray="2 3"
  />
  <line
    x1={size / 2} y1={size * 0.1} x2={size / 2} y2={size * 0.9}
    class="stroke-border" stroke-width="1" stroke-dasharray="2 3"
  />

  {#each EDGES as [a, b], i}
    <line
      x1={pts[a][0]} y1={pts[a][1]} x2={pts[b][0]} y2={pts[b][1]}
      class="stroke-foreground" stroke-width="1.25" stroke-linecap="round"
      opacity={edgeOpacity(i)}
    />
  {/each}

  <line
    x1={noseBase[0]} y1={noseBase[1]} x2={noseTip[0]} y2={noseTip[1]}
    class="stroke-primary" stroke-width="2" stroke-linecap="round"
  />
  <circle cx={noseTip[0]} cy={noseTip[1]} r="2.5" class="fill-primary" />
</svg>
