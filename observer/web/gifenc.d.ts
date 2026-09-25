declare module "gifenc" {
  type Palette = number[][];
  type GIF = {
    bytes(): Uint8Array;
    finish(): void;
    writeFrame(
      pixels: Uint8Array,
      width: number,
      height: number,
      options: { delay?: number; palette?: Palette; repeat?: number },
    ): void;
  };

  export function GIFEncoder(): GIF;
  export function quantize(
    pixels: Uint8Array | Uint8ClampedArray,
    colors: number,
    options?: { format?: "rgb565" | "rgb444" | "rgba4444" },
  ): Palette;
  export function applyPalette(
    pixels: Uint8Array | Uint8ClampedArray,
    palette: Palette,
    format?: "rgb565" | "rgb444" | "rgba4444",
  ): Uint8Array;
}
