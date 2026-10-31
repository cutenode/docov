/**
 * @module lib
 */

/** Adds. */
function add(a, b) {
  return a + b;
}

function subtract(a, b) {
  return a - b;
}

/**
 * Called when it's done.
 * @callback Done
 * @param {Error} [error]
 */

/** Runs a task. */
exports.run = function (task, done) {
  done();
};

exports.stop = function () {};

module.exports.add = add;
module.exports.subtract = subtract;
