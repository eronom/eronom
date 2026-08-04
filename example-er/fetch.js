async function dataFetch() {
    const data = await fetch("https://jsonplaceholder.typicode.com/todos/1")
    const response = await data.json()
    console.log(response)
}
dataFetch()