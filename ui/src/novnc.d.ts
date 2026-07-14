// Minimal typing for the noVNC RFB client — the package ships no
// TypeScript declarations (its root export is core/rfb.js).
declare module "@novnc/novnc" {
  export default class RFB {
    constructor(
      target: HTMLElement,
      url: string,
      options?: { credentials?: { password?: string } },
    );
    disconnect(): void;
    scaleViewport: boolean;
    viewOnly: boolean;
    addEventListener(name: string, handler: (ev: CustomEvent) => void): void;
  }
}
