/**
 * Minimal typings for `d3-force-3d`, which ships none.
 *
 * It mirrors `d3-force`'s API and adds `numDimensions`, so the same simulation code serves both
 * view modes — one dependency instead of a 2D one and a 3D one.
 */
declare module 'd3-force-3d' {
  export interface SimNode {
    x: number
    y: number
    z?: number
    vx?: number
    vy?: number
    vz?: number
    index?: number
  }

  export interface Force<N> {
    (alpha: number): void
    initialize?: (nodes: N[]) => void
  }

  export interface Simulation<N extends SimNode> {
    nodes(): N[]
    force(name: string, force: unknown): this
    alpha(a: number): this
    alphaDecay(d: number): this
    alphaMin(m: number): this
    numDimensions(n: number): this
    tick(iterations?: number): this
    stop(): this
    restart(): this
    on(event: 'tick' | 'end', listener: () => void): this
  }

  export interface ManyBodyForce<N> extends Force<N> {
    strength(s: number): this
    theta(t: number): this
    distanceMax(d: number): this
  }

  export interface LinkForce<N, L> extends Force<N> {
    /** d3 keys its link map by whatever this returns — a number is as valid as a string, and the
     *  worker uses the array index precisely to keep strings off the thread boundary. */
    id(fn: (node: N) => string | number): this
    distance(d: number): this
    strength(s: number): this
    links(links: L[]): this
  }

  export interface CollideForce<N> extends Force<N> {
    radius(fn: (node: N) => number): this
    iterations(n: number): this
  }

  export function forceSimulation<N extends SimNode>(
    nodes?: N[],
    numDimensions?: number,
  ): Simulation<N>
  export function forceManyBody<N extends SimNode>(): ManyBodyForce<N>
  export function forceLink<N extends SimNode, L>(links?: L[]): LinkForce<N, L>
  export function forceCollide<N extends SimNode>(): CollideForce<N>
  export interface AxisForce<N> extends Force<N> {
    strength(s: number): this
  }
  export function forceX<N extends SimNode>(x?: number): AxisForce<N>
  export function forceY<N extends SimNode>(y?: number): AxisForce<N>
  export function forceZ<N extends SimNode>(z?: number): AxisForce<N>

  export function forceCenter<N extends SimNode>(
    x?: number,
    y?: number,
    z?: number,
  ): Force<N>
}
