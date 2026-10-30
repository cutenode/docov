/**
 * A point in the plane.
 * @typedef {Object} Point
 * @property {number} x
 * @property {number} y
 */

/**
 * How far a point is from the origin.
 * @param {Point} p The point.
 * @returns {number}
 */
export function length(p) {
  return Math.hypot(p.x, p.y);
}

/** @typedef {number} Size */
export function area(width, height) {
  return width * height;
}

/** A circle. */
export class Circle {
  /** @param {number} r The radius. */
  constructor(r) {
    this.r = r;
  }

  /** Its area. */
  area() {
    return Math.PI * this.r ** 2;
  }

  scale(by) {
    return new Circle(this.r * by);
  }

  /** Its diameter. */
  get diameter() {
    return this.r * 2;
  }
}

/** The unit circle. */
export const unit = new Circle(1);

export const origin = { x: 0, y: 0 };

function helper() {}

/** Documented, but not exported. */
function documentedHelper() {}
