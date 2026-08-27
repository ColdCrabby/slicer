export type RuntimeErrorCode =
  'not_ready' | 'unsupported' | 'transport_error' | 'invalid_request' | 'internal_error';

export interface RuntimeError {
  code: RuntimeErrorCode;
  message: string;
  cause?: unknown;
}

export function normalizeRuntimeError(
  cause: unknown,
  fallbackCode: RuntimeErrorCode,
): RuntimeError {
  if (cause instanceof Error) {
    return {
      code: fallbackCode,
      message: cause.message,
      cause,
    };
  }

  return {
    code: fallbackCode,
    message: 'Unknown runtime error',
    cause,
  };
}
