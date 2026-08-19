const res = await fetch("http://localhost:5223/script/run", {
  method: "POST",
  headers: {
    "Content-Type": "application/json",
  },
  body: Bun.file("weapon_select.json"),
});

console.log(res);

export {};
