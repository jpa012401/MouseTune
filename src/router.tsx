import { createHashRouter, Navigate } from "react-router";
import { Layout } from "./components/Layout";
import { Placeholder } from "./components/Placeholder";
import { About } from "./routes/About";

// Hash routing: the settings window loads from a single bundled index.html.
export function createAppRouter() {
  return createHashRouter([
    {
      path: "/",
      element: <Layout />,
      children: [
        { index: true, element: <Navigate to="/buttons" replace /> },
        { path: "buttons", element: <Placeholder title="Buttons" phase="Phase 4" /> },
        { path: "scroll", element: <Placeholder title="Scroll" phase="Phase 5" /> },
        { path: "general", element: <Placeholder title="General" phase="Phase 7" /> },
        { path: "about", element: <About /> },
      ],
    },
  ]);
}
