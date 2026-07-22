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

function addTerrain(root: THREE.Group, state: SausageSceneState, palette: (typeof PALETTES)[number]) {
  const solidTiles = state.tiles.filter((tile) => tile.kind !== "spectral_sausage");
  const cube = new THREE.BoxGeometry(1, 1, 1);
  const side = material(palette.side);
  const top = material(palette.top);
  const bottom = material(palette.bottom);
  const terrain = new THREE.InstancedMesh(cube, [side, side, top, bottom, side, side], solidTiles.length);
  terrain.name = "terrain";
  terrain.receiveShadow = true;
  const matrix = new THREE.Matrix4();
  solidTiles.forEach((tile, index) => {
    matrix.makeTranslation(...worldPosition(tile.pos, true).toArray());
    terrain.setMatrixAt(index, matrix);
  });
  terrain.instanceMatrix.needsUpdate = true;
  root.add(terrain);

  const topLines: number[] = [];
  for (const tile of solidTiles) {
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
    new THREE.LineBasicMaterial({ color: palette.grid, transparent: true, opacity: 0.44 }),
  ));

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

function addSausage(root: THREE.Group, entity: SceneEntity, changed: boolean, spectral = false) {
  const group = new THREE.Group();
  group.name = `sausage-${entity.id}`;
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
    emissive: changed ? 0xffd85a : 0x000000,
    emissiveIntensity: changed ? 0.22 : 0,
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

function dispose(root: THREE.Object3D) {
  root.traverse((object) => {
    if (!(object instanceof THREE.Mesh || object instanceof THREE.LineSegments)) return;
    object.geometry.dispose();
    const materials = Array.isArray(object.material) ? object.material : [object.material];
    materials.forEach((entry) => entry.dispose());
  });
}

export class SausageScene {
  private readonly renderer: THREE.WebGLRenderer;
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera(60, 1, 0.1, 500);
  private readonly controls: OrbitControls;
  private readonly resizeObserver: ResizeObserver;
  private root = new THREE.Group();
  private state: SausageSceneState | null = null;
  private view: "player" | "overview" = "player";

  constructor(private readonly canvas: HTMLCanvasElement) {
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, powerPreference: "high-performance" });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.renderer.shadowMap.enabled = true;
    this.renderer.shadowMap.type = THREE.PCFShadowMap;
    this.controls = new OrbitControls(this.camera, canvas);
    this.controls.enableDamping = false;
    this.controls.enablePan = true;
    this.controls.enableRotate = true;
    this.controls.enableZoom = true;
    this.controls.screenSpacePanning = true;
    this.controls.minDistance = 3;
    this.controls.maxDistance = 220;
    this.controls.minPolarAngle = 0.12;
    this.controls.maxPolarAngle = Math.PI * 0.49;
    this.controls.addEventListener("change", () => this.render());
    this.scene.add(this.root);
    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(canvas);
    this.resize();
  }

  update(state: SausageSceneState, changedIds: Set<number>) {
    const previous = this.state;
    this.state = state;
    this.scene.remove(this.root);
    dispose(this.root);
    this.root = new THREE.Group();
    const palette = PALETTES[state.tileSet] ?? PALETTES[0];
    this.scene.background = new THREE.Color(palette.fog);
    this.scene.fog = new THREE.Fog(palette.fog, 28, 80);
    addTerrain(this.root, state, palette);
    for (const entity of state.entities) {
      if (entity.kind === "player") addPlayer(this.root, entity);
      else if (entity.kind === "sausage") addSausage(this.root, entity, changedIds.has(entity.id));
      else if (entity.kind === "spectral_sausage") addSausage(this.root, entity, changedIds.has(entity.id), true);
      else if (entity.kind === "fork") addDetachedFork(this.root, entity);
    }
    if (state.exit) addExit(this.root, state.exit);
    this.addWorldFloor(state, palette);
    this.addLights(state);
    this.scene.add(this.root);
    if (!previous || previous.levelKey !== state.levelKey) {
      this.view = "player";
      this.focusPlayer();
    } else if (this.view === "player") {
      this.followPlayer(previous, state);
    }
    this.render();
  }

  focusPlayer() {
    if (!this.state) return;
    this.view = "player";
    this.frame(activityPoints(this.state), playerPoint(this.state));
    this.render();
  }

  showOverview() {
    if (!this.state) return;
    this.view = "overview";
    this.frame(scenePoints(this.state));
    this.render();
  }

  private addWorldFloor(state: SausageSceneState, palette: (typeof PALETTES)[number]) {
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
    this.root.add(floor);
    const grid = new THREE.GridHelper(size, Math.max(8, Math.round(size)), palette.grid, palette.grid);
    grid.position.copy(floor.position);
    grid.position.y += 0.006;
    const gridMaterial = grid.material as THREE.LineBasicMaterial;
    gridMaterial.transparent = true;
    gridMaterial.opacity = 0.12;
    this.root.add(grid);
  }

  private addLights(state: SausageSceneState) {
    const points = scenePoints(state);
    if (!points.length) return;
    const bounds = new THREE.Box3().setFromPoints(points);
    const center = bounds.getCenter(new THREE.Vector3());
    const size = bounds.getSize(new THREE.Vector3());
    const hemisphere = new THREE.HemisphereLight(0xe9f4ff, 0x35403d, 2.15);
    this.root.add(hemisphere);
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
    this.root.add(sun.target, sun);
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
    const width = Math.max(1, this.canvas.clientWidth);
    const height = Math.max(1, this.canvas.clientHeight);
    this.renderer.setSize(width, height, false);
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
    this.render();
  }

  private render() {
    this.renderer.render(this.scene, this.camera);
  }

  destroy() {
    this.resizeObserver.disconnect();
    this.controls.dispose();
    dispose(this.root);
    this.renderer.dispose();
    this.state = null;
  }
}
