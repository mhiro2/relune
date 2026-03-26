{
  const svg = document.querySelector('.canvas svg');
  if (svg) {
    svg.querySelectorAll('.edge').forEach((edge, index) => {
      (edge as SVGElement).style.setProperty('--enter-index', String(index));
    });

    svg.querySelectorAll('.node').forEach((node, index) => {
      (node as SVGElement).style.setProperty('--enter-index', String(index + 6));
    });
  }
}
