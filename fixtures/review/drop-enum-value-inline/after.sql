CREATE TABLE orders (
    id BIGINT PRIMARY KEY,
    status ENUM('pending', 'paid', 'shipped') NOT NULL
) ENGINE=InnoDB;
