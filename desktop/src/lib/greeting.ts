/** Time-of-day greeting ("Good morning, Natsumi"). Single source of truth —
 *  Home and the Library masthead used to carry their own copies (one of them
 *  hardcoded to "Good afternoon"), which is how the greeting froze. */
export function greeting(name: string): string {
  const h = new Date().getHours();
  if (h < 5) return `Late night, ${name}`;
  if (h < 12) return `Good morning, ${name}`;
  if (h < 18) return `Good afternoon, ${name}`;
  return `Good evening, ${name}`;
}
