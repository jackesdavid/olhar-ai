import { redirect } from "@sveltejs/kit";

// Faces is the 2D canvas now — the grid view is retired (#119). `/faces` → the canvas.
export const load = () => {
  throw redirect(307, "/faces/canvas");
};
