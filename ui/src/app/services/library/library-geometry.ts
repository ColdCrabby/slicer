import {
  Box3,
  BufferAttribute,
  BufferGeometry,
  DirectionalLight,
  Group,
  HemisphereLight,
  Mesh,
  MeshStandardMaterial,
  PerspectiveCamera,
  Scene,
  Sphere,
  Vector3,
  WebGLRenderer,
} from 'three';
import { SceneHandle } from '../../../generated/scene-wasm/scene_engine';

/**
 * Three.js for the library: turning a model file into geometry, and geometry
 * into a thumbnail or a live preview.
 *
 * **Loaded on demand, never at startup.** three's ESM build is one module, so a
 * static import from the library service would put all of it in front of the
 * first screen. {@link ObjectLibrary} and the preview reach this file through
 * `import()`.
 *
 * **Parsing goes through the engine.** A throwaway `SceneHandle` reads the file
 * with the same loaders — and the same import-time repair — as the plate, so a
 * thumbnail shows exactly the object that will land on the bed. The scene
 * engine must already be initialised; the caller awaits `SceneEngine.ready()`.
 */

/** A bed big enough that nothing is refused for not fitting; nothing is placed on it. */
const PARSE_BED = {
  width: 10_000,
  depth: 10_000,
  height: 10_000,
  origin_offset_x: 0,
  origin_offset_y: 0,
};

/** Thumbnails are square and stored at this many pixels a side. */
export const THUMBNAIL_SIZE = 320;

/**
 * One mid tone, between the viewer's light- and dark-theme model colours. A
 * thumbnail is rendered once and kept, so it has to read on either theme.
 */
const MODEL_COLOR = 0xaab1bd;

/** Camera direction for thumbnails and a preview's starting pose — the viewer's isometric preset. */
const VIEW_DIRECTION = new Vector3(1, -1, 0.8).normalize();

/** Parse a model file into one geometry per part, in the file's own coordinates. */
export function parseModel(fileName: string, bytes: Uint8Array): BufferGeometry[] {
  const format = fileName.toLowerCase().split('.').pop() ?? 'stl';
  const handle = new SceneHandle(PARSE_BED);
  try {
    const ids = handle.addMesh(fileName, format, bytes, undefined);
    return Array.from(ids, (id) => {
      const buffer = handle.getRenderBuffer(id);
      const geometry = new BufferGeometry();
      geometry.setAttribute('position', new BufferAttribute(buffer.positions, 3));
      geometry.setAttribute('normal', new BufferAttribute(buffer.normals, 3));
      buffer.free();
      return geometry;
    });
  } finally {
    handle.free();
  }
}

/** A lit scene holding the model, centred on the origin, plus its bounding sphere. */
export function studio(geometries: readonly BufferGeometry[]): {
  scene: Scene;
  model: Group;
  radius: number;
  dispose: () => void;
} {
  const scene = new Scene();
  const material = new MeshStandardMaterial({
    color: MODEL_COLOR,
    roughness: 0.55,
    metalness: 0.05,
  });
  const model = new Group();
  for (const geometry of geometries) {
    model.add(new Mesh(geometry, material));
  }
  const box = new Box3().setFromObject(model);
  const centre = box.getCenter(new Vector3());
  model.position.sub(centre);
  const radius = Math.max(box.getBoundingSphere(new Sphere()).radius, 1e-3);
  scene.add(model);

  scene.add(new HemisphereLight(0xffffff, 0x9a9ea8, 1.4));
  const key = new DirectionalLight(0xffffff, 1.6);
  key.position.set(1, -2, 3);
  scene.add(key);
  const fill = new DirectionalLight(0xffffff, 0.5);
  fill.position.set(-2, 1, 1);
  scene.add(fill);

  return {
    scene,
    model,
    radius,
    dispose: () => {
      geometries.forEach((g) => g.dispose());
      material.dispose();
    },
  };
}

/** Place `camera` on `direction` so a sphere of `radius` fills the frame. */
export function frame(camera: PerspectiveCamera, radius: number, direction = VIEW_DIRECTION): void {
  // A bounding sphere is loose around most models; a little under its radius
  // fills the square without clipping the silhouette.
  const distance = (radius * 0.9) / Math.sin((camera.fov * Math.PI) / 360);
  camera.up.set(0, 0, 1);
  camera.position.copy(direction).multiplyScalar(distance);
  camera.near = distance / 100;
  camera.far = distance * 4;
  camera.lookAt(0, 0, 0);
  camera.updateProjectionMatrix();
}

let sharedRenderer: WebGLRenderer | null = null;

/**
 * Render a model file to a transparent PNG.
 *
 * One renderer serves every thumbnail, reused for the life of the page: a
 * browser allows only a handful of WebGL contexts, and a library of hundreds of
 * models must not spend one per card. That is the whole reason the grid shows
 * pictures and only the selected model gets a live 3D view.
 */
export async function renderThumbnail(fileName: string, bytes: Uint8Array): Promise<Blob> {
  const geometries = parseModel(fileName, bytes);
  const { scene, radius, dispose } = studio(geometries);
  try {
    sharedRenderer ??= new WebGLRenderer({
      antialias: true,
      alpha: true,
      preserveDrawingBuffer: true,
    });
    const renderer = sharedRenderer;
    renderer.setPixelRatio(1);
    renderer.setSize(THUMBNAIL_SIZE, THUMBNAIL_SIZE, false);
    renderer.setClearColor(0x000000, 0);
    const camera = new PerspectiveCamera(30, 1);
    frame(camera, radius);
    renderer.render(scene, camera);
    return await new Promise<Blob>((resolve, reject) =>
      renderer.domElement.toBlob(
        (blob) => (blob ? resolve(blob) : reject(new Error('thumbnail capture failed'))),
        'image/png',
      ),
    );
  } finally {
    dispose();
  }
}
