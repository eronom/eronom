

const start = async () => {
  console.log("Starting sequential fetches (Bun)...")
  await fetch("https://jsonplaceholder.typicode.com/todos/1")
  await fetch("https://jsonplaceholder.typicode.com/todos/2")
  await fetch("https://jsonplaceholder.typicode.com/todos/3")
  await fetch("https://jsonplaceholder.typicode.com/todos/4")
  await fetch("https://jsonplaceholder.typicode.com/todos/5")
  console.log("Finished sequential fetches.")
}

await start()
