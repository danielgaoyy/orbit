// Orbit Desktop Root Entrypoint (System-Adaptive Theme + Error Boundary)
import { Component, type ErrorInfo, type ReactNode, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import App from "./App.tsx";

class RootErrorBoundary extends Component<
  { children: ReactNode },
  { error: Error | null }
> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Orbit UI boundary caught error:", error, info);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="bg-background text-foreground flex h-screen w-screen flex-col items-center justify-center gap-3 p-6 text-[13px]">
          <div className="text-[14px] font-semibold text-rose-500">Orbit UI Render Error</div>
          <pre className="bg-muted max-w-xl overflow-auto rounded-lg p-3.5 font-mono text-[12px]">
            {this.state.error.message}
          </pre>
          <button
            type="button"
            onClick={() => window.location.reload()}
            className="bg-foreground text-background cursor-pointer rounded-lg px-3.5 py-2 text-[13px] font-medium hover:opacity-90"
          >
            Reload Window
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <RootErrorBoundary>
      <App />
    </RootErrorBoundary>
  </StrictMode>,
);
