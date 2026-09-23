function classes(...values) {
  return values.filter((value) => typeof value === "string" && value.length > 0).join(" ");
}
export {
  classes
};
