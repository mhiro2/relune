CREATE TABLE orders (
    id BIGINT PRIMARY KEY,
    status ENUM('pending', 'paid', 'cancelled', 'shipped') NOT NULL
) ENGINE=InnoDB;
