// Node.js (JavaScript) Equivalent
const taskA = async (url) => {
    console.log("Starting fetch A...");
    await fetch(url);
    console.log("Completed fetch A!");
};
const taskB = async (url) => {
    console.log("Starting fetch B...");
    await fetch(url);
    console.log("Completed fetch B!");
};
// Equivalent to Eronom's `concurrent { on taskA(...) on taskB(...) }`
await Promise.all([
    taskA("https://jsonplaceholder.typicode.com/todos/1"),
    taskB("https://jsonplaceholder.typicode.com/todos/2")
]);
console.log("Both tasks completed!");