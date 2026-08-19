const res = await fetch("http://localhost:5223/script/run", {
  method: "POST",
  headers: {
    "Content-Type": "application/json",
  },
  body: Bun.file("ar_mode.json"),
});

console.log(res);

export {};
