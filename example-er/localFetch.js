const taskA = async (url) => {
    await fetch(url);
};

const taskB = async (url) => {
    await fetch(url);
};

await Promise.all([
    taskA("http://127.0.0.1:8989/"),
    taskB("http://127.0.0.1:8989/")
]);

console.log("Done!");
