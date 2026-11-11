/**
 * Converts a temperature from Celsius to Fahrenheit
 *
 * @param celsius - The temperature in degrees Celsius.
 * @returns The temperature in degrees Fahrenheit.
 */
export function toFahrenheit(celsius: number): number {
  return (celsius * 9) / 5 + 32;
}
