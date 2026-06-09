let name = "Vishnu";
let age = 25;

let player = {
  name: name,
  age: age,
  printInfo: (self) => {
    console.log(`${self.name} is ${self.age} years old`);
  }
};

player.printInfo(player);
