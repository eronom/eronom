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

// Sequential Execution in JavaScript:
// taskB will NOT start until taskA is completely finished!
await taskA("https://jsonplaceholder.typicode.com/todos/1");
await taskB("https://jsonplaceholder.typicode.com/todos/2");

console.log("Both tasks completed!");
