let balance = 100;

const withdraw = async (amount, customerName) => {
  console.log(`[${customerName}] Checking balance...`);
  
  if (balance >= amount) {
    console.log(`[${customerName}] Balance check passed! Simulating processing delay...`);
    
    // Explicit yield point (await)
    await fetch("https://jsonplaceholder.typicode.com/todos/1");
    
    balance = balance - amount;
    console.log(`[${customerName}] Withdrawal complete! New balance: ${balance}`);
  } else {
    console.log(`[${customerName}] Insufficient funds!`);
  }
};

console.log(`Initial Balance: ${balance}`);

// Fire both asynchronously (without awaiting immediately) to run them concurrently
const p1 = withdraw(80, "Customer A");
const p2 = withdraw(80, "Customer B");

// Await both promises to complete
await Promise.all([p1, p2]);

console.log(`Final Balance: ${balance}`);
