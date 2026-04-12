import { Component, type ReactNode, type ErrorInfo } from "react";

interface Props {
  name: string;
  children: ReactNode;
}

interface State {
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  override state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  override componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(`[ErrorBoundary:${this.props.name}]`, error, info.componentStack);
  }

  override render() {
    if (this.state.error) {
      return (
        <div className="error-boundary">
          <span className="error-boundary-label">{this.props.name}</span>
          <span className="error-boundary-msg">{this.state.error.message}</span>
        </div>
      );
    }
    return this.props.children;
  }
}
