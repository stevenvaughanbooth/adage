/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Where the teaser films are served from, ending in "/" (e.g. a Vercel Blob store). Default: /media/. */
  readonly VITE_MEDIA_BASE?: string;
  /** "off" builds the site without the opening film. */
  readonly VITE_FILM?: string;
  /** The public repository, for the README and lexicon links. Unset: the links are hidden. */
  readonly VITE_REPO_URL?: string;
}
declare module "./hero/scene.js" {
  export function build(opts: { canvas: HTMLCanvasElement; labels: HTMLElement; onLost: () => void; light: boolean; held: boolean; reduced: boolean }): unknown;
}
