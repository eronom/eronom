let balance = 100;

const withdraw = async (amount, customerName) => {
  console.log(`[${customerName}] Checking balance...`);
  
  if (balance >= amount) {
    // 1. Deduct synchronously before yielding control to the await expression
    balance = balance - amount;
    console.log(`[${customerName}] Balance check passed! Funds reserved. New balance: ${balance}`);
    
    console.log(`[${customerName}] Processing payment...`);
    // 2. Yield control (fetch)
    await fetch("https://jsonplaceholder.typicode.com/todos/1");
    
    console.log(`[${customerName}] Withdrawal complete!`);
  } else {
    console.log(`[${customerName}] Insufficient funds!`);
  }
};

console.log(`Initial Balance: ${balance}`);

// Fire both concurrently
const p1 = withdraw(80, "Customer A");
const p2 = withdraw(80, "Customer B");

await Promise.all([p1, p2]);

console.log(`Final Balance: ${balance}`);
