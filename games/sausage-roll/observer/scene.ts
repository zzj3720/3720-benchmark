import type { GameState } from "../../../observer/web/app/game-observer";
import { WebGLSurface } from "../../../observer/web/app/webgl/runtime";
import { sausageHud } from "./webgl";
import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";

import type { Coord3, SausageSceneState, SceneEntity, SceneTile } from "./scene-state";

const PALETTES = [
  { top: 0x78965a, side: 0x4e6946, bottom: 0x34493b, outside: 0x29414b, grid: 0xb7cc8c, fog: 0x7796a3 },
  { top: 0xb89a63, side: 0x796746, bottom: 0x514b3c, outside: 0x344752, grid: 0xe0c990, fog: 0x93a5aa },
  { top: 0xbecbd0, side: 0x82949d, bottom: 0x586d78, outside: 0x607a8f, grid: 0xf1f7f7, fog: 0xa9c3d5 },
  { top: 0x65745b, side: 0x414d43, bottom: 0x2b3531, outside: 0x3e3937, grid: 0x9ba083, fog: 0x786e67 },
  { top: 0x8a8795, side: 0x605e70, bottom: 0x424152, outside: 0x293444, grid: 0xc6c0d2, fog: 0x77869a },
] as const;

const COOK_COLORS = [
  new THREE.Color(0xe98468),
  new THREE.Color(0xa95d38),
  new THREE.Color(0x75412e),
  new THREE.Color(0x271d1a),
];

function worldPosition(pos: Coord3, terrain = false) {
  return new THREE.Vector3(pos.x + 0.5, pos.z + (terrain ? 0.5 : 0), -pos.y - 0.5);
}

function scenePoints(state: SausageSceneState) {
  return [
    ...state.tiles.map((tile) => worldPosition(tile.pos, true)),
    ...state.entities.map((entity) => worldPosition(entity.pos)),
  ];
}

/** Longest tween; faster replays shorten it to fit between frames. */
const MOTION_MS = 260;

type Placement = { position: THREE.Vector3; quaternion: THREE.Quaternion; rotation?: number };
type MotionItem = { object: THREE.Object3D; from: Placement; to: Placement; hop: boolean; spin?: { mesh?: THREE.Object3D; from: number } };
type Motion = { start: number; duration: number; items: MotionItem[]; camera?: { from: { position: THREE.Vector3; target: THREE.Vector3 }; to: { position: THREE.Vector3; target: THREE.Vector3 } } };

function placementOf(object: THREE.Object3D): Placement {
  return { position: object.position.clone(), quaternion: object.quaternion.clone(), rotation: object.userData.rotation as number | undefined };
}

function playerPoint(state: SausageSceneState) {
  const player = state.entities.find((entity) => entity.kind === "player");
  return player ? worldPosition(player.pos).add(new THREE.Vector3(0, 0.72, 0)) : null;
}

function activityPoints(state: SausageSceneState) {
  const player = playerPoint(state);
  if (!player) return scenePoints(state);
  const nearby = scenePoints(state).filter((point) => {
    const horizontal = Math.hypot(point.x - player.x, point.z - player.z);
    return horizontal <= 7 && Math.abs(point.y - player.y) <= 5;
  });
  nearby.push(player);
  return nearby;
}

function terrainTop(pos: Coord3) {
  return worldPosition(pos, true).y + 0.5;
}

function directionVector(direction: string) {
  switch (direction) {
    case "north": return new THREE.Vector3(0, 0, 1);
    case "south": return new THREE.Vector3(0, 0, -1);
    case "west": return new THREE.Vector3(-1, 0, 0);
    case "east": return new THREE.Vector3(1, 0, 0);
    default: return new THREE.Vector3(0, 0, 1);
  }
}

function orientForward(group: THREE.Object3D, direction: string) {
  const forward = directionVector(direction);
  group.rotation.y = Math.atan2(forward.x, forward.z);
}

function material(color: number, roughness = 0.82) {
  return new THREE.MeshStandardMaterial({ color, roughness, metalness: 0.02 });
}

function addTerrain(root: THREE.Group, state: SausageSceneState) {
  const solidTiles = state.tiles.filter((tile) => tile.kind !== "spectral_sausage");
  for (const tileSet of new Set(solidTiles.map((tile) => tile.tileSet))) {
    const palette = PALETTES[tileSet] ?? PALETTES[0];
    const tiles = solidTiles.filter((tile) => tile.tileSet === tileSet);
    const side = material(palette.side);
    const top = material(palette.top);
    const bottom = material(palette.bottom);
    const terrain = new THREE.InstancedMesh(
      new THREE.BoxGeometry(1, 1, 1),
      [side, side, top, bottom, side, side],
      tiles.length,
    );
    terrain.name = `terrain-${tileSet}`;
    terrain.receiveShadow = true;
    const matrix = new THREE.Matrix4();
    tiles.forEach((tile, index) => {
      matrix.makeTranslation(...worldPosition(tile.pos, true).toArray());
      terrain.setMatrixAt(index, matrix);
    });
    terrain.instanceMatrix.needsUpdate = true;
    root.add(terrain);

    const topLines: number[] = [];
    for (const tile of tiles) {
      const { x, y, z } = worldPosition(tile.pos, true);
      const h = y + 0.501;
      topLines.push(
        x - 0.5, h, z - 0.5, x + 0.5, h, z - 0.5,
        x + 0.5, h, z - 0.5, x + 0.5, h, z + 0.5,
        x + 0.5, h, z + 0.5, x - 0.5, h, z + 0.5,
        x - 0.5, h, z + 0.5, x - 0.5, h, z - 0.5,
      );
    }
    const lineGeometry = new THREE.BufferGeometry();
    lineGeometry.setAttribute("position", new THREE.Float32BufferAttribute(topLines, 3));
    root.add(new THREE.LineSegments(
      lineGeometry,
      new THREE.LineBasicMaterial({ color: palette.grid, transparent: true, opacity: 0.38 }),
    ));
  }

  for (const tile of state.tiles) {
    if (tile.kind === "grill") addGrill(root, tile);
    if (tile.kind === "ladder") addLadder(root, tile);
  }
}

function addGrill(root: THREE.Group, tile: SceneTile) {
  const group = new THREE.Group();
  group.position.copy(worldPosition(tile.pos));
  group.position.y = terrainTop(tile.pos) + 0.035;
  orientForward(group, tile.direction);
  const plate = new THREE.Mesh(new THREE.BoxGeometry(0.84, 0.07, 0.84), material(0x282b2c, 0.55));
  plate.receiveShadow = true;
  group.add(plate);
  const glow = new THREE.MeshStandardMaterial({ color: 0xff9d38, emissive: 0xff5c1f, emissiveIntensity: 0.9, roughness: 0.5 });
  for (let index = -3; index <= 3; index += 1) {
    const slat = new THREE.Mesh(new THREE.BoxGeometry(0.7, 0.035, 0.055), glow);
    slat.position.set(0, 0.06, index * 0.105);
    group.add(slat);
  }
  root.add(group);
}

function addLadder(root: THREE.Group, tile: SceneTile) {
  const group = new THREE.Group();
  const forward = directionVector(tile.direction);
  group.position.copy(worldPosition(tile.pos));
  group.position.addScaledVector(forward, 0.46);
  group.position.y += 0.48;
  orientForward(group, tile.direction);
  const ladderMaterial = material(0xe8c477, 0.58);
  for (const x of [-0.27, 0.27]) {
    const rail = new THREE.Mesh(new THREE.BoxGeometry(0.055, 1.08, 0.055), ladderMaterial);
    rail.position.x = x;
    rail.castShadow = true;
    group.add(rail);
  }
  for (let index = -2; index <= 2; index += 1) {
    const rung = new THREE.Mesh(new THREE.BoxGeometry(0.58, 0.045, 0.06), ladderMaterial);
    rung.position.y = index * 0.2;
    rung.castShadow = true;
    group.add(rung);
  }
  root.add(group);
}

function addPlayer(root: THREE.Group, entity: SceneEntity) {
  const group = new THREE.Group();
  group.name = `player-${entity.id}`;
  group.position.copy(worldPosition(entity.pos));
  orientForward(group, entity.direction);
  const skin = material(0xffcf42, 0.62);
  const shirt = material(0xd33f43, 0.72);
  const dark = material(0x282423, 0.64);
  const body = new THREE.Mesh(new THREE.BoxGeometry(0.42, 0.68, 0.36), shirt);
  body.position.y = 0.68;
  const head = new THREE.Mesh(new THREE.IcosahedronGeometry(0.31, 1), skin);
  head.position.y = 1.22;
  const leftFoot = new THREE.Mesh(new THREE.BoxGeometry(0.17, 0.18, 0.31), dark);
  const rightFoot = leftFoot.clone();
  leftFoot.position.set(-0.14, 0.1, 0.03);
  rightFoot.position.set(0.14, 0.1, 0.03);
  for (const mesh of [body, head, leftFoot, rightFoot]) {
    mesh.castShadow = true;
    group.add(mesh);
  }
  const marker = new THREE.Mesh(new THREE.ConeGeometry(0.11, 0.28, 3), material(0xffffff, 0.5));
  marker.rotation.x = Math.PI / 2;
  marker.position.set(0, 1.03, 0.34);
  group.add(marker);
  if (entity.cells.length > 1) {
    const heldFork = new THREE.Group();
    heldFork.position.set(0, 0.72, 0.5);
    addFork(heldFork);
    group.add(heldFork);
  }
  root.add(group);
}

function addFork(parent: THREE.Group) {
  const forkMaterial = new THREE.MeshStandardMaterial({ color: 0x343b3d, roughness: 0.38, metalness: 0.62 });
  const handle = new THREE.Mesh(new THREE.BoxGeometry(0.075, 0.075, 0.5), forkMaterial);
  handle.position.z = -0.14;
  handle.castShadow = true;
  parent.add(handle);
  for (const x of [-0.17, 0, 0.17]) {
    const tine = new THREE.Mesh(new THREE.BoxGeometry(0.055, 0.055, 0.28), forkMaterial);
    tine.position.set(x, 0, 0.25);
    tine.castShadow = true;
    parent.add(tine);
  }
}

function sausageGeometry(faces: number[], rotation: number) {
  const geometry = new THREE.CapsuleGeometry(0.36, 1.08, 7, 24);
  const normals = geometry.getAttribute("normal");
  const positions = geometry.getAttribute("position");
  const colors = new Float32Array(normals.count * 3);
  for (let index = 0; index < normals.count; index += 1) {
    const firstHalf = positions.getY(index) < 0;
    const top = normals.getZ(index) < 0;
    const face = rotation
      ? (firstHalf ? (top ? faces[3] : faces[2]) : (top ? faces[0] : faces[1]))
      : (firstHalf ? (top ? faces[2] : faces[3]) : (top ? faces[1] : faces[0]));
    const color = COOK_COLORS[Math.max(0, Math.min(3, face))];
    color.toArray(colors, index * 3);
  }
  geometry.setAttribute("color", new THREE.BufferAttribute(colors, 3));
  return geometry;
}

function addSausage(root: THREE.Group, entity: SceneEntity, spectral = false) {
  const group = new THREE.Group();
  group.name = `sausage-${entity.id}`;
  group.userData.rotation = entity.rotation;
  const forward = directionVector(entity.direction);
  group.position.copy(worldPosition(entity.pos));
  group.position.addScaledVector(forward, 0.5);
  group.position.y += 0.43;
  orientForward(group, entity.direction);
  const axis = new THREE.Group();
  axis.rotation.x = Math.PI / 2;
  group.add(axis);
  const sausageMaterial = new THREE.MeshStandardMaterial({
    vertexColors: true,
    roughness: 0.74,
    metalness: 0,
    transparent: spectral,
    opacity: spectral ? 0.48 : 1,
  });
  const faces = entity.cookedFaces ?? [0, 0, 0, 0];
  const sausage = new THREE.Mesh(sausageGeometry(faces, entity.rotation), sausageMaterial);
  sausage.castShadow = true;
  sausage.receiveShadow = true;
  axis.add(sausage);
  root.add(group);
}

function addDetachedFork(root: THREE.Group, entity: SceneEntity) {
  const group = new THREE.Group();
  group.name = `fork-${entity.id}`;
  group.position.copy(worldPosition(entity.pos));
  group.position.y += 0.12;
  orientForward(group, entity.direction);
  addFork(group);
  root.add(group);
}

function addExit(root: THREE.Group, exit: NonNullable<SausageSceneState["exit"]>) {
  const group = new THREE.Group();
  group.position.copy(worldPosition(exit.pos));
  orientForward(group, exit.direction);
  const color = exit.ready ? 0xa7ff78 : 0x8b9693;
  const pad = new THREE.Mesh(
    new THREE.CylinderGeometry(0.34, 0.42, 0.055, 20),
    new THREE.MeshStandardMaterial({ color, emissive: color, emissiveIntensity: exit.ready ? 0.45 : 0.05, roughness: 0.7 }),
  );
  pad.position.y = 0.035;
  group.add(pad);
  const arrow = new THREE.Mesh(new THREE.ConeGeometry(0.17, 0.4, 4), material(color, 0.5));
  arrow.rotation.x = Math.PI / 2;
  arrow.position.set(0, 0.34, 0.12);
  group.add(arrow);
  root.add(group);
}

function addEntrances(root: THREE.Group, state: SausageSceneState) {
  for (const entrance of state.entrances) {
    const group = new THREE.Group();
    group.name = `entrance-${entrance.ordinal}`;
    group.position.copy(worldPosition(entrance.pos));
    orientForward(group, entrance.direction);
    const available = entrance.status === "available";
    const color = available ? 0xffd15c : 0x8bdc78;
    const ring = new THREE.Mesh(
      new THREE.TorusGeometry(available ? 0.28 : 0.18, available ? 0.06 : 0.045, 6, 18),
      new THREE.MeshStandardMaterial({
        color,
        emissive: color,
        emissiveIntensity: available ? 0.62 : 0.14,
        roughness: 0.62,
      }),
    );
    ring.rotation.x = -Math.PI / 2;
    ring.position.y = 0.06;
    group.add(ring);
    if (available) {
      const arrowMaterial = new THREE.MeshStandardMaterial({
        color,
        emissive: color,
        emissiveIntensity: 0.7,
        roughness: 0.55,
      });
      const shaft = new THREE.Mesh(new THREE.BoxGeometry(0.11, 0.055, 0.62), arrowMaterial);
      shaft.position.set(0, 0.11, 0.18);
      const head = new THREE.Mesh(new THREE.ConeGeometry(0.2, 0.38, 4), arrowMaterial);
      head.rotation.x = Math.PI / 2;
      head.position.set(0, 0.11, 0.55);
      group.add(shaft, head);
      const beacon = new THREE.Mesh(
        new THREE.CylinderGeometry(0.025, 0.025, 1.4, 6),
        new THREE.MeshBasicMaterial({ color, transparent: true, opacity: 0.55 }),
      );
      beacon.position.y = 0.75;
      group.add(beacon);
    }
    root.add(group);
  }
}

function dispose(root: THREE.Object3D) {
  root.traverse((object) => {
    if (!(object instanceof THREE.Mesh || object instanceof THREE.LineSegments)) return;
    object.geometry.dispose();
    const materials = Array.isArray(object.material) ? object.material : [object.material];
    materials.forEach((entry) => entry.dispose());
  });
}

function staticSignature(state: SausageSceneState) {
  let hash = 2166136261;
  const mix = (value: number | string) => {
    const text = String(value);
    for (let index = 0; index < text.length; index += 1) {
      hash ^= text.charCodeAt(index);
      hash = Math.imul(hash, 16777619);
    }
  };
  mix(state.levelKey);
  mix(state.tileSet);
  for (const tile of state.tiles) {
    mix(tile.pos.x); mix(tile.pos.y); mix(tile.pos.z);
    mix(tile.kind); mix(tile.direction); mix(tile.tileSet); mix(tile.variant);
  }
  for (const entrance of state.entrances) {
    mix(entrance.ordinal); mix(entrance.pos.x); mix(entrance.pos.y); mix(entrance.pos.z);
    mix(entrance.direction); mix(entrance.status);
  }
  return `${state.levelKey}:${hash >>> 0}`;
}

export class SausageScene {
  private readonly renderer: THREE.WebGLRenderer;
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera(60, 1, 0.1, 500);
  private readonly controls: OrbitControls;
  private readonly resizeObserver: ResizeObserver;
  private staticRoot = new THREE.Group();
  private dynamicRoot = new THREE.Group();
  private staticKey = "";
  private state: SausageSceneState | null = null;
  private overlay: WebGLSurface | null = null;
  private rawState: GameState = {};
  private width = 1;
  private height = 1;
  private readonly contextLost = (event: Event) => { event.preventDefault(); this.canvas.dataset.ready = "false"; };
  private readonly contextRestored = () => { window.requestAnimationFrame(() => this.render()); };
  private view: "player" | "overview" = "player";
  /** The tween in flight, and where the camera is heading so a new move adds to its goal, not to a mid-tween position. */
  private motion: Motion | null = null;
  private motionFrame = 0;
  private lastUpdate = 0;
  private cameraGoal: { position: THREE.Vector3; target: THREE.Vector3 } | null = null;
  /** Whether the last "player" framing found the player to centre on. */
  private anchored = false;

  constructor(private readonly canvas: HTMLCanvasElement, private readonly fixed?: { width: number; height: number; resolution: number }) {
    this.renderer = new THREE.WebGLRenderer({ canvas, stencil: true, preserveDrawingBuffer: true, antialias: true, powerPreference: "high-performance" });
    this.renderer.setPixelRatio(fixed?.resolution ?? Math.min(window.devicePixelRatio, 2));
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.renderer.shadowMap.enabled = true;
    this.renderer.shadowMap.type = THREE.PCFShadowMap;
    canvas.addEventListener("webglcontextlost", this.contextLost);
    canvas.addEventListener("webglcontextrestored", this.contextRestored);
    this.controls = new OrbitControls(this.camera, canvas);
    this.controls.enableDamping = false;
    this.controls.enablePan = true;
    this.controls.enableRotate = true;
    this.controls.enableZoom = true;
    this.controls.screenSpacePanning = true;
    this.controls.minDistance = 3;
    this.controls.maxDistance = 500;
    this.controls.minPolarAngle = 0.12;
    this.controls.maxPolarAngle = Math.PI * 0.49;
    this.controls.addEventListener("change", () => this.render());
    this.scene.add(this.staticRoot, this.dynamicRoot);
    this.resizeObserver = new ResizeObserver(() => this.resize());
    if (!fixed) this.resizeObserver.observe(canvas);
    this.controls.enabled = !fixed;
    this.resize();
  }

  async initializeOverlay() {
    this.overlay = await WebGLSurface.create(this.canvas, this.width, this.height, this.renderer.getPixelRatio(), this.renderer.getContext() as WebGL2RenderingContext);
    this.render();
  }

  copyViewFrom(source: SausageScene) {
    // Replay export starts at its first frame, while the visible canvas may
    // already show another level or the overworld after a completed puzzle.
    if (!this.state || this.state.levelKey !== source.state?.levelKey) return;
    this.view = source.view;
    this.camera.position.copy(source.camera.position);
    this.camera.quaternion.copy(source.camera.quaternion);
    this.camera.near = source.camera.near;
    this.camera.far = source.camera.far;
    this.camera.updateProjectionMatrix();
    this.controls.target.copy(source.controls.target);
    const from = source.state && playerPoint(source.state), to = this.state && playerPoint(this.state);
    if (this.view === "player" && from && to) {
      const delta = to.sub(from); this.camera.position.add(delta); this.controls.target.add(delta);
    }
    this.render();
  }

  update(state: SausageSceneState, rawState: GameState = {}) {
    this.rawState = rawState;
    const previous = this.state;
    this.state = state;
    const now = performance.now(), interval = now - this.lastUpdate;
    this.lastUpdate = now;
    window.cancelAnimationFrame(this.motionFrame);
    // Exports capture one frame per state and reduced motion asks for none.
    const animate = !this.fixed && previous?.levelKey === state.levelKey && !window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    const shown = new Map<string, Placement>();
    if (animate) for (const child of this.dynamicRoot.children) if (child.name) shown.set(child.name, placementOf(child));
    const palette = PALETTES[state.tileSet] ?? PALETTES[0];
    const nextStaticKey = staticSignature(state);
    if (nextStaticKey !== this.staticKey) {
      this.staticKey = nextStaticKey;
      this.scene.remove(this.staticRoot);
      dispose(this.staticRoot);
      this.staticRoot = new THREE.Group();
      this.scene.background = new THREE.Color(palette.fog);
      const points = state.tiles.map((tile) => worldPosition(tile.pos, true));
      const size = points.length
        ? new THREE.Box3().setFromPoints(points).getSize(new THREE.Vector3())
        : new THREE.Vector3();
      const span = Math.max(size.x, size.z);
      this.scene.fog = new THREE.Fog(palette.fog, Math.max(28, span * 0.55), Math.max(80, span * 1.8));
      addTerrain(this.staticRoot, state);
      if (state.mode === "overworld") addEntrances(this.staticRoot, state);
      this.addWorldFloor(this.staticRoot, state, palette);
      this.addLights(this.staticRoot, state);
      this.scene.add(this.staticRoot);
    }
    this.scene.remove(this.dynamicRoot);
    dispose(this.dynamicRoot);
    this.dynamicRoot = new THREE.Group();
    for (const entity of state.entities) {
      if (entity.kind === "player") addPlayer(this.dynamicRoot, entity);
      else if (entity.kind === "sausage") addSausage(this.dynamicRoot, entity);
      else if (entity.kind === "spectral_sausage") addSausage(this.dynamicRoot, entity, true);
      else if (entity.kind === "fork") addDetachedFork(this.dynamicRoot, entity);
    }
    if (state.exit) addExit(this.dynamicRoot, state.exit);
    this.scene.add(this.dynamicRoot);
    const items: MotionItem[] = [];
    for (const child of this.dynamicRoot.children) {
      const from = shown.get(child.name);
      if (!from) continue;
      const to = placementOf(child);
      const rolled = from.rotation !== undefined && from.rotation !== to.rotation;
      if (from.position.distanceTo(to.position) < 1e-3 && from.quaternion.angleTo(to.quaternion) < 1e-3 && !rolled) continue;
      // More than about a cell sideways in one frame was not one step; sliding would show a path never taken. Falls are vertical and still tween.
      if (Math.hypot(to.position.x - from.position.x, to.position.z - from.position.z) > 1.6) continue;
      const item: MotionItem = { object: child, from, to, hop: child.name.startsWith("player-") };
      if (rolled) {
        // The new colours already show the rolled sausage; spin into them about its long axis.
        const forward = new THREE.Vector3(0, 0, 1).applyQuaternion(to.quaternion), delta = to.position.clone().sub(from.position);
        item.spin = { mesh: child.children[0]?.children[0], from: Math.PI * (Math.sign(forward.z * delta.x - forward.x * delta.z) || 1) };
      }
      items.push(item);
    }
    if (!items.length && this.cameraGoal) {
      // An interrupted follow that this update does not continue lands where it was heading.
      this.camera.position.copy(this.cameraGoal.position);
      this.controls.target.copy(this.cameraGoal.target);
      this.controls.update();
      this.cameraGoal = null;
    }
    let camera: Motion["camera"];
    if (!previous || previous.levelKey !== state.levelKey) {
      // Follow the player everywhere, including the overworld: the whole map
      // is too large to read at once. "完整地图" still shows everything.
      this.view = "player";
      this.focusPlayer();
    } else if (this.view === "player") {
      // A first frame without a player (or before the map loaded) cannot be
      // centred; centre as soon as the player appears instead of panning.
      if (!this.anchored) this.focusPlayer();
      else if (animate && items.length) camera = this.cameraMove(previous, state);
      else this.followPlayer(previous, state);
    }
    if (items.length || camera) {
      this.motion = { start: now, duration: Math.max(90, Math.min(MOTION_MS, interval * 0.75)), items, camera };
      this.step();
    } else {
      this.motion = null;
      this.render();
    }
  }

  private readonly step = () => {
    const motion = this.motion;
    if (!motion) return;
    const t = Math.min(1, (performance.now() - motion.start) / motion.duration);
    const e = t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2;
    for (const item of motion.items) {
      item.object.position.lerpVectors(item.from.position, item.to.position, e);
      if (item.hop) item.object.position.y += Math.sin(Math.PI * e) * 0.18;
      item.object.quaternion.slerpQuaternions(item.from.quaternion, item.to.quaternion, e);
      if (item.spin?.mesh) item.spin.mesh.rotation.y = item.spin.from * (1 - e);
    }
    if (motion.camera) {
      this.camera.position.lerpVectors(motion.camera.from.position, motion.camera.to.position, e);
      this.controls.target.lerpVectors(motion.camera.from.target, motion.camera.to.target, e);
      this.controls.update();
    }
    this.render();
    if (t < 1) this.motionFrame = window.requestAnimationFrame(this.step);
    else { this.motion = null; this.cameraGoal = null; }
  };

  /** The camera tween for a follow: from where it is to where the last goal plus this move puts it. */
  private cameraMove(previous: SausageSceneState, state: SausageSceneState): Motion["camera"] {
    const before = playerPoint(previous), after = playerPoint(state);
    if (!before || !after) { this.focusPlayer(); return undefined; }
    const delta = after.sub(before);
    const from = { position: this.camera.position.clone(), target: this.controls.target.clone() };
    const base = this.cameraGoal ?? from;
    this.cameraGoal = { position: base.position.clone().add(delta), target: base.target.clone().add(delta) };
    return { from, to: this.cameraGoal };
  }

  focusPlayer() {
    if (!this.state) return;
    this.cameraGoal = null;
    this.view = "player";
    const player = playerPoint(this.state);
    this.anchored = Boolean(player);
    this.frame(activityPoints(this.state), player);
    this.render();
  }

  showOverview() {
    if (!this.state) return;
    this.cameraGoal = null;
    this.view = "overview";
    this.frame(scenePoints(this.state));
    this.render();
  }

  prepareExportFrame() {
    this.render();
  }

  private addWorldFloor(root: THREE.Group, state: SausageSceneState, palette: (typeof PALETTES)[number]) {
    const points = state.tiles.map((tile) => worldPosition(tile.pos, true));
    if (!points.length) return;
    const minY = Math.min(...points.map((point) => point.y - 0.5));
    const box = new THREE.Box3().setFromPoints(points);
    const size = Math.max(box.max.x - box.min.x, box.max.z - box.min.z) + 24;
    const floor = new THREE.Mesh(
      new THREE.PlaneGeometry(size, size),
      new THREE.MeshStandardMaterial({ color: palette.outside, roughness: 0.92, metalness: 0.03 }),
    );
    floor.rotation.x = -Math.PI / 2;
    floor.position.set((box.min.x + box.max.x) / 2, minY - 0.04, (box.min.z + box.max.z) / 2);
    floor.receiveShadow = true;
    root.add(floor);
    const grid = new THREE.GridHelper(size, Math.max(8, Math.round(size)), palette.grid, palette.grid);
    grid.position.copy(floor.position);
    grid.position.y += 0.006;
    const gridMaterial = grid.material as THREE.LineBasicMaterial;
    gridMaterial.transparent = true;
    gridMaterial.opacity = 0.12;
    root.add(grid);
  }

  private addLights(root: THREE.Group, state: SausageSceneState) {
    const points = state.tiles.map((tile) => worldPosition(tile.pos, true));
    if (!points.length) return;
    const bounds = new THREE.Box3().setFromPoints(points);
    const center = bounds.getCenter(new THREE.Vector3());
    const size = bounds.getSize(new THREE.Vector3());
    const hemisphere = new THREE.HemisphereLight(0xe9f4ff, 0x35403d, 2.15);
    root.add(hemisphere);
    const sun = new THREE.DirectionalLight(0xfff0cf, 3.1);
    sun.position.copy(center).add(new THREE.Vector3(-8, 16, -10));
    sun.target.position.copy(center);
    sun.castShadow = true;
    sun.shadow.mapSize.set(1024, 1024);
    const shadowSpan = Math.max(size.x, size.z, 10) * 0.72;
    sun.shadow.camera.left = -shadowSpan;
    sun.shadow.camera.right = shadowSpan;
    sun.shadow.camera.top = shadowSpan;
    sun.shadow.camera.bottom = -shadowSpan;
    root.add(sun.target, sun);
  }

  private followPlayer(previous: SausageSceneState, state: SausageSceneState) {
    const before = playerPoint(previous);
    const after = playerPoint(state);
    if (!before || !after) {
      this.focusPlayer();
      return;
    }
    const delta = after.sub(before);
    this.camera.position.add(delta);
    this.controls.target.add(delta);
    this.controls.update();
  }

  private frame(points: THREE.Vector3[], anchor?: THREE.Vector3 | null) {
    if (!points.length) return;
    const bounds = new THREE.Box3().setFromPoints(points);
    bounds.expandByScalar(1.4);
    const center = bounds.getCenter(new THREE.Vector3());
    const size = bounds.getSize(new THREE.Vector3());
    const verticalFov = THREE.MathUtils.degToRad(this.camera.fov);
    const horizontalFov = 2 * Math.atan(Math.tan(verticalFov / 2) * this.camera.aspect);
    const distance = Math.max(
      size.x / (2 * Math.tan(horizontalFov / 2)),
      size.z / (2 * Math.tan(verticalFov / 2)),
      size.y * 1.35,
      6,
    ) * 1.32;
    const target = anchor
      ? anchor.clone().lerp(center, 0.24)
      : center.clone().add(new THREE.Vector3(0, Math.min(1.2, size.y * 0.12), 0));
    this.camera.position.copy(target).add(new THREE.Vector3(0, distance * 0.93, -distance * 0.38));
    this.camera.lookAt(target);
    this.camera.near = Math.max(0.1, distance / 120);
    this.camera.far = Math.max(120, distance * 8);
    this.camera.updateProjectionMatrix();
    this.controls.target.copy(target);
    this.controls.update();
  }

  private resize() {
    const width = this.fixed?.width ?? Math.max(1, this.canvas.clientWidth);
    const height = this.fixed?.height ?? Math.max(1, this.canvas.clientHeight);
    const changed = width !== this.width || height !== this.height;
    this.width = width; this.height = height;
    this.renderer.setSize(width, height, false);
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
    // Framing depends on the aspect ratio; redo it once the canvas has its size.
    if (changed && this.state) {
      if (this.view === "player") this.focusPlayer();
      else this.showOverview();
    }
    this.render();
  }

  private render() {
    const hud = this.state ? sausageHud(this.rawState, this.state, this.width, this.height) : null;
    const bottom = hud?.footer ?? 28, fieldHeight = Math.max(1, this.height - 66 - bottom);
    this.camera.aspect = this.width / fieldHeight;
    this.camera.updateProjectionMatrix();
    this.renderer.resetState();
    this.renderer.setViewport(0, bottom, this.width, fieldHeight);
    this.renderer.render(this.scene, this.camera);
    if (hud && this.overlay) this.overlay.render(hud.scene, { clear: false });
  }

  destroy() {
    window.cancelAnimationFrame(this.motionFrame);
    this.motion = null;
    this.canvas.removeEventListener("webglcontextlost", this.contextLost);
    this.canvas.removeEventListener("webglcontextrestored", this.contextRestored);
    this.resizeObserver.disconnect();
    this.controls.dispose();
    dispose(this.staticRoot);
    dispose(this.dynamicRoot);
    this.overlay?.destroy();
    this.overlay = null;
    this.renderer.dispose();
    this.renderer.forceContextLoss();
    this.state = null;
  }
}
