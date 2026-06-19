// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IAccountKeychain} from "../src/interfaces/IAccountKeychain.sol";

interface Vm {
    function envUint(string calldata name) external returns (uint256);
    function envAddress(string calldata name) external returns (address);
    function envOr(string calldata name, uint256 defaultValue) external returns (uint256);
    function startBroadcast(uint256 privateKey) external;
    function stopBroadcast() external;
}

contract AuthorizeSessionKey {
    address internal constant ACCOUNT_KEYCHAIN = 0xaAAAaaAA00000000000000000000000000000000;
    bytes4 internal constant TIP20_TRANSFER_SELECTOR = bytes4(keccak256("transfer(address,uint256)"));
    Vm internal constant VM = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));

    function run() external {
        uint256 rootPrivateKey = VM.envUint("ROOT_PRIVATE_KEY");
        address sessionKey = VM.envAddress("SESSION_KEY");
        address tokenX = VM.envAddress("TOKEN_X");
        address recipient = VM.envAddress("RECIPIENT");
        uint256 spendLimit = VM.envOr("SPEND_LIMIT", uint256(100 ether));
        uint64 period = _envOrUint64("PERIOD", 0);

        uint256 configuredExpiry = VM.envOr("EXPIRY", uint256(0));
        uint256 resolvedExpiry = configuredExpiry == 0 ? block.timestamp + 7 days : configuredExpiry;
        require(resolvedExpiry <= type(uint64).max, "EXPIRY_TOO_LARGE");
        // forge-lint: disable-next-line(unsafe-typecast)
        uint64 expiry = uint64(resolvedExpiry);

        IAccountKeychain.TokenLimit[] memory limits = new IAccountKeychain.TokenLimit[](1);
        limits[0] = IAccountKeychain.TokenLimit({token: tokenX, amount: spendLimit, period: period});

        address[] memory recipients = new address[](1);
        recipients[0] = recipient;

        IAccountKeychain.SelectorRule[] memory selectorRules = new IAccountKeychain.SelectorRule[](1);
        selectorRules[0] = IAccountKeychain.SelectorRule({
            selector: TIP20_TRANSFER_SELECTOR,
            recipients: recipients
        });

        IAccountKeychain.CallScope[] memory allowedCalls = new IAccountKeychain.CallScope[](1);
        allowedCalls[0] = IAccountKeychain.CallScope({target: tokenX, selectorRules: selectorRules});

        IAccountKeychain.KeyRestrictions memory config = IAccountKeychain.KeyRestrictions({
            expiry: expiry,
            enforceLimits: true,
            limits: limits,
            allowAnyCalls: false,
            allowedCalls: allowedCalls
        });

        VM.startBroadcast(rootPrivateKey);
        IAccountKeychain(ACCOUNT_KEYCHAIN).authorizeKey(
            sessionKey,
            IAccountKeychain.SignatureType.Secp256k1,
            config
        );
        VM.stopBroadcast();
    }

    function _envOrUint64(string memory name, uint64 defaultValue) internal returns (uint64) {
        uint256 value = VM.envOr(name, uint256(defaultValue));
        require(value <= type(uint64).max, "UINT64_ENV_TOO_LARGE");
        // forge-lint: disable-next-line(unsafe-typecast)
        return uint64(value);
    }
}
