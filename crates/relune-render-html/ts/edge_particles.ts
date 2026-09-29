/** Classes under which the viewer emphasizes an edge and shows its flow particle. */
const EMPHASIS_CLASSES = ['highlighted-neighbor', 'hover-preview-edge'];

/** The subset of `SVGAnimationElement` used here; DOM shims may omit the timing methods. */
interface TimedAnimation extends Element {
  beginElement?: () => void;
  endElement?: () => void;
}

export interface EdgeParticles {
  /** Starts or stops each edge's particle to match its hover and emphasis state. */
  sync(): void;
}

/**
 * Takes over the FK flow particles from the SVG's own hover timing.
 *
 * The SVG starts a particle on `mouseenter` of its edge, which cannot follow
 * the viewer's highlight classes. The viewer instead begins and ends each
 * animation itself, so a particle runs only while its edge is hovered or
 * emphasized and nothing animates while idle.
 */
export function createEdgeParticles(svgRoot: Element): EdgeParticles {
  const edges = new Map<Element, TimedAnimation>();
  const hovered = new Set<Element>();
  const running = new Set<Element>();

  const syncEdge = (edge: Element, motion: TimedAnimation): void => {
    const active =
      hovered.has(edge) || EMPHASIS_CLASSES.some((name) => edge.classList.contains(name));
    if (active === running.has(edge)) return;
    if (active) {
      running.add(edge);
      motion.beginElement?.();
    } else {
      running.delete(edge);
      motion.endElement?.();
    }
  };

  svgRoot.querySelectorAll('.edge').forEach((edge) => {
    const motion: TimedAnimation | null = edge.querySelector('animateMotion');
    if (motion === null) return;
    motion.setAttribute('begin', 'indefinite');
    motion.removeAttribute('end');
    edges.set(edge, motion);

    edge.addEventListener('mouseenter', () => {
      hovered.add(edge);
      syncEdge(edge, motion);
    });
    edge.addEventListener('mouseleave', () => {
      hovered.delete(edge);
      syncEdge(edge, motion);
    });
  });

  return {
    sync(): void {
      edges.forEach((motion, edge) => {
        syncEdge(edge, motion);
      });
    },
  };
}
