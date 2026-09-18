function fastParams(speeds: Record<string, number | string>): Record<string, unknown> {
  return {
    ...defaultProcessParams(),
    // Derived from the nozzle rather than pinned at 0.44: these are the
    // machines least likely to be running a 0.4.
    line_width: 0,
    ...speeds,
  };
}
