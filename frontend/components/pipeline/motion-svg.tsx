'use client';

import { Component, type ReactNode, type RefObject, type SVGProps } from 'react';
import type { Scene } from '@/lib/pipeline/layout';
import {
  Animator,
  SceneMotion,
  browserClock,
  type Departing,
  type MotionCtx,
  type MotionView,
} from '@/lib/pipeline/motion';

interface MotionSvgProps {
  /** Attributes of the <svg> element itself. */
  attrs: Omit<SVGProps<SVGSVGElement>, 'ref' | 'children'>;
  svgRef: RefObject<SVGSVGElement | null>;
  scene: Scene;
  ctx: MotionCtx;
  reduced: boolean;
  view: MotionView;
  children: ReactNode;
}

const NO_DEPARTURES: Departing = { cards: new Map(), squares: new Map() };

/**
 * The pipeline SVG with its animator. React renders the target geometry as children; after each
 * commit the animator moves every node from what it showed to that target. A class component, for
 * `getSnapshotBeforeUpdate`: departing elements are cloned before React removes them, so they can
 * animate out.
 */
export class MotionSvg extends Component<MotionSvgProps, object, Departing | null> {
  private motion: SceneMotion | null = null;

  componentDidMount(): void {
    const svg = this.props.svgRef.current;
    if (!svg) throw new Error('MotionSvg: the svg element is not mounted');
    this.motion = new SceneMotion(new Animator(browserClock, this.props.reduced), this.props.view);
    this.motion.sync(svg, null, this.props.scene, this.props.ctx, NO_DEPARTURES);
  }

  getSnapshotBeforeUpdate(prev: MotionSvgProps): Departing | null {
    const svg = this.props.svgRef.current;
    if (!svg || !this.motion || prev.scene === this.props.scene) return null;
    return this.motion.captureDeparting(svg, prev.scene, this.props.scene);
  }

  componentDidUpdate(prev: MotionSvgProps, _state: object, departing: Departing | null): void {
    const svg = this.props.svgRef.current;
    if (!svg || !this.motion) return;
    if (prev.reduced !== this.props.reduced) this.motion.setReduced(this.props.reduced);
    if (prev.scene !== this.props.scene || prev.ctx !== this.props.ctx) {
      this.motion.sync(
        svg,
        prev.scene,
        this.props.scene,
        this.props.ctx,
        departing ?? NO_DEPARTURES
      );
    }
  }

  componentWillUnmount(): void {
    this.motion?.dispose();
    this.motion = null;
  }

  render() {
    const { svgRef, attrs, children } = this.props;
    return (
      <svg ref={svgRef} {...attrs}>
        {children}
        <g data-layer="exit" pointerEvents="none" />
        <g data-layer="fx" pointerEvents="none" />
      </svg>
    );
  }
}
