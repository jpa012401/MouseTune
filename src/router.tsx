import { createHashRouter, Navigate } from "react-router";
import { Layout } from "./components/Layout";
import { Buttons } from "./routes/Buttons";
import { General } from "./routes/General";
import { Scroll } from "./routes/Scroll";
import { About } from "./routes/About";

// Hash routing: the settings window loads from a single bundled index.html.
export function createAppRouter() {
  return createHashRouter([
    {
      path: "/",
      element: <Layout />,
      children: [
        { index: true, element: <Navigate to="/buttons" replace /> },
        { path: "buttons", element: <Buttons /> },
        { path: "scroll", element: <Scroll /> },
        { path: "general", element: <General /> },
        { path: "about", element: <About /> },
      ],
    },
  ]);
}
