declare global {
  // React 19 uses this flag to enable act() warnings in test environments.
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

export {};
