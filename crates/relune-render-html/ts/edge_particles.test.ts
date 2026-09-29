import { beforeEach, describe, expect, it, vi } from 'vitest';

import { createEdgeParticles } from './edge_particles';

// Mirrors the FK edge markup emitted by relune-render-svg.
function renderEdges(): SVGSVGElement {
  document.body.innerHTML = `<svg>
    <g id="edge-0" class="edge"><path id="edge-path-0" class="edge-path"/>
      <g class="edge-particles"><circle class="edge-particle"><animateMotion begin="edge-0.mouseenter" end="edge-0.mouseleave"/></circle></g>
    </g>
    <g id="edge-1" class="edge"><path id="edge-path-1" class="edge-path"/></g>
  </svg>`;
  const svg = document.querySelector('svg');
  if (svg === null) throw new Error('missing svg');
  return svg;
}

function motionOf(edgeId: string): Element & { beginElement: () => void; endElement: () => void } {
  const motion = document.querySelector(`#${edgeId} animateMotion`);
  if (motion === null) throw new Error(`missing animation for ${edgeId}`);
  return Object.assign(motion, { beginElement: vi.fn(), endElement: vi.fn() });
}

describe('createEdgeParticles', () => {
  let svg: SVGSVGElement;

  beforeEach(() => {
    svg = renderEdges();
  });

  it('replaces the SVG hover timing with script-driven timing', () => {
    createEdgeParticles(svg);
    const motion = document.querySelector('#edge-0 animateMotion');
    expect(motion?.getAttribute('begin')).toBe('indefinite');
    expect(motion?.hasAttribute('end')).toBe(false);
  });

  it('runs a particle while its edge is emphasized and stops it afterwards', () => {
    const motion = motionOf('edge-0');
    const particles = createEdgeParticles(svg);
    const edge = document.getElementById('edge-0');

    particles.sync();
    expect(motion.beginElement).not.toHaveBeenCalled();

    edge?.classList.add('highlighted-neighbor');
    particles.sync();
    particles.sync();
    expect(motion.beginElement).toHaveBeenCalledTimes(1);

    edge?.classList.remove('highlighted-neighbor');
    particles.sync();
    expect(motion.endElement).toHaveBeenCalledTimes(1);
  });

  it('keeps an emphasized particle running when the pointer leaves the edge', () => {
    const motion = motionOf('edge-0');
    const particles = createEdgeParticles(svg);
    const edge = document.getElementById('edge-0');

    edge?.dispatchEvent(new Event('mouseenter'));
    expect(motion.beginElement).toHaveBeenCalledTimes(1);

    edge?.classList.add('hover-preview-edge');
    particles.sync();
    edge?.dispatchEvent(new Event('mouseleave'));
    expect(motion.endElement).not.toHaveBeenCalled();

    edge?.classList.remove('hover-preview-edge');
    particles.sync();
    expect(motion.endElement).toHaveBeenCalledTimes(1);
  });
});
