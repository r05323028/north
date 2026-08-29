import next from "eslint-config-next";

// eslint-config-next ships a native flat config (includes its own ignores).
const eslintConfig = [...next, { ignores: ["coverage/**"] }];

export default eslintConfig;
